//! Optional runtime model-catalog capability for drivable AI tools.
//!
//! The transport advertises the capability through [`Drive::model_catalog`]; callers
//! never branch on a provider id. Runtime model ids and effort levels are authoritative
//! when discovery succeeds, while plugin manifests retain stable labels and provide the
//! complete fallback when discovery is unsupported or unavailable.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use hotsheet_plugins::{AiToolDescriptor, ModelSpec, Plugin};

use crate::host::drive_for;

/// One model reported by a live provider runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeModelSpec {
    pub id: String,
    pub label: String,
    pub effort_levels: Vec<String>,
    pub default_effort: Option<String>,
    pub is_default: bool,
}

/// The runtime-owned portion of an AI tool's model catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeModelCatalog {
    pub models: Vec<RuntimeModelSpec>,
}

/// Optional live capability exposed by a drive transport.
pub trait RuntimeModelCatalogSource {
    /// Stable runtime version/fingerprint used to invalidate the cache after upgrades.
    fn version(&self) -> Result<String, String>;
    /// Query the installed runtime's current catalog.
    fn discover(&self, cwd: &Path) -> Result<RuntimeModelCatalog, String>;
}

/// A plugin-declared CLI catalog such as `opencode models` or `agy models`.
#[derive(Debug, Clone)]
pub struct CommandModelCatalog {
    program: String,
    args: Vec<String>,
    effort_levels: Vec<String>,
    default_effort: Option<String>,
}

impl CommandModelCatalog {
    pub fn new(
        program: String,
        args: Vec<String>,
        effort_levels: Vec<String>,
        default_effort: Option<String>,
    ) -> Self {
        Self {
            program,
            args,
            effort_levels,
            default_effort,
        }
    }

    fn output(&self, args: &[String], cwd: Option<&Path>) -> Result<String, String> {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let output = command
            .output()
            .map_err(|error| format!("starting '{}': {error}", self.program))?;
        if !output.status.success() {
            return Err(format!(
                "'{} {}' exited with {}",
                self.program,
                args.join(" "),
                output.status
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl RuntimeModelCatalogSource for CommandModelCatalog {
    fn version(&self) -> Result<String, String> {
        let args = vec!["--version".to_string()];
        let value = self.output(&args, None)?.trim().to_string();
        (!value.is_empty())
            .then_some(value)
            .ok_or_else(|| format!("'{} --version' returned no version", self.program))
    }

    fn discover(&self, cwd: &Path) -> Result<RuntimeModelCatalog, String> {
        let output = self.output(&self.args, Some(cwd))?;
        Ok(parse_command_catalog(
            &output,
            &self.effort_levels,
            self.default_effort.as_deref(),
        ))
    }
}

fn parse_command_catalog(
    output: &str,
    effort_levels: &[String],
    default_effort: Option<&str>,
) -> RuntimeModelCatalog {
    RuntimeModelCatalog {
        models: output
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                let (id, label) = line
                    .split_once('\t')
                    .map_or((line, line), |(id, label)| (id.trim(), label.trim()));
                (!id.is_empty()).then(|| RuntimeModelSpec {
                    id: id.to_string(),
                    label: if label.is_empty() { id } else { label }.to_string(),
                    effort_levels: effort_levels.to_vec(),
                    default_effort: default_effort.map(str::to_string),
                    is_default: false,
                })
            })
            .collect(),
    }
}

#[derive(Debug, Clone)]
struct CachedCatalog {
    version: String,
    catalog: Option<RuntimeModelCatalog>,
}

/// Per-process cache. A failed lookup is cached for its runtime version as well, so an
/// unavailable provider cannot turn client refreshes into repeated process launches.
#[derive(Debug, Default)]
pub struct ModelCatalogCache {
    entries: HashMap<String, CachedCatalog>,
}

impl ModelCatalogCache {
    /// Resolve one manifest descriptor through an optional runtime capability.
    pub fn resolve(
        &mut self,
        manifest: AiToolDescriptor,
        source: Option<&dyn RuntimeModelCatalogSource>,
        cwd: &Path,
        refresh: bool,
    ) -> AiToolDescriptor {
        let Some(source) = source else {
            return manifest;
        };
        let Some(version) = source
            .version()
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            return self
                .entries
                .get(&manifest.id)
                .and_then(|entry| entry.catalog.as_ref())
                .map_or(manifest.clone(), |catalog| {
                    merge_runtime_catalog(manifest, catalog)
                });
        };
        let existing = self.entries.get(&manifest.id).cloned();
        if !refresh
            && existing
                .as_ref()
                .is_some_and(|entry| entry.version == version)
        {
            return existing
                .and_then(|entry| entry.catalog)
                .map_or(manifest.clone(), |catalog| {
                    merge_runtime_catalog(manifest, &catalog)
                });
        }

        match source.discover(cwd) {
            Ok(catalog) if !catalog.models.is_empty() => {
                self.entries.insert(
                    manifest.id.clone(),
                    CachedCatalog {
                        version,
                        catalog: Some(catalog.clone()),
                    },
                );
                merge_runtime_catalog(manifest, &catalog)
            }
            _ if existing
                .as_ref()
                .is_some_and(|entry| entry.version == version && entry.catalog.is_some()) =>
            {
                merge_runtime_catalog(manifest, existing.unwrap().catalog.as_ref().unwrap())
            }
            _ => {
                self.entries.insert(
                    manifest.id.clone(),
                    CachedCatalog {
                        version,
                        catalog: None,
                    },
                );
                manifest
            }
        }
    }
}

/// Discover all installed drivable plugins, enriching only those whose drive exposes a
/// runtime catalog capability. Registry order remains stable.
pub fn discover_ai_tool_descriptors(
    search_dirs: &[PathBuf],
    cwd: &Path,
    cache: &mut ModelCatalogCache,
    refresh: bool,
) -> Vec<AiToolDescriptor> {
    hotsheet_plugins::detected_drivable_plugins(search_dirs)
        .into_iter()
        .filter_map(|plugin| resolve_plugin(&plugin, cwd, cache, refresh))
        .collect()
}

fn resolve_plugin(
    plugin: &Plugin,
    cwd: &Path,
    cache: &mut ModelCatalogCache,
    refresh: bool,
) -> Option<AiToolDescriptor> {
    let manifest = hotsheet_plugins::ai_tool_descriptor(plugin)?;
    let drive = drive_for(plugin);
    Some(
        cache.resolve(
            manifest,
            drive
                .as_deref()
                .and_then(crate::drive::Drive::model_catalog),
            cwd,
            refresh,
        ),
    )
}

fn merge_runtime_catalog(
    mut descriptor: AiToolDescriptor,
    runtime: &RuntimeModelCatalog,
) -> AiToolDescriptor {
    let declared = descriptor
        .models
        .iter()
        .map(|model| (model.id.as_str(), model))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let models = runtime
        .models
        .iter()
        .filter(|model| !model.id.trim().is_empty() && seen.insert(model.id.as_str()))
        .map(|model| {
            let fallback = declared.get(model.id.as_str());
            ModelSpec {
                id: model.id.clone(),
                label: fallback
                    .map(|model| model.label.clone())
                    .filter(|label| !label.trim().is_empty())
                    .or_else(|| (!model.label.trim().is_empty()).then(|| model.label.clone()))
                    .unwrap_or_else(|| model.id.clone()),
                effort_levels: model.effort_levels.clone(),
            }
        })
        .collect::<Vec<_>>();
    if models.is_empty() {
        return descriptor;
    }

    let runtime_default = runtime
        .models
        .iter()
        .find(|model| model.is_default && models.iter().any(|item| item.id == model.id));
    let default_model = runtime_default
        .map(|model| model.id.clone())
        .or_else(|| {
            descriptor
                .default_model
                .clone()
                .filter(|id| models.iter().any(|model| &model.id == id))
        })
        .or_else(|| models.first().map(|model| model.id.clone()));
    let default_effort = default_model
        .as_deref()
        .and_then(|id| runtime.models.iter().find(|model| model.id == id))
        .and_then(|model| model.default_effort.clone())
        .filter(|effort| {
            models
                .iter()
                .find(|model| Some(model.id.as_str()) == default_model.as_deref())
                .is_some_and(|model| model.effort_levels.contains(effort))
        })
        .or_else(|| {
            descriptor.default_effort.clone().filter(|effort| {
                models
                    .iter()
                    .find(|model| Some(model.id.as_str()) == default_model.as_deref())
                    .is_some_and(|model| model.effort_levels.contains(effort))
            })
        });
    descriptor.models = models;
    descriptor.default_model = default_model;
    descriptor.default_effort = default_effort;
    descriptor
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    struct FakeCatalog {
        version: RefCell<Result<String, String>>,
        catalog: RefCell<Result<RuntimeModelCatalog, String>>,
        calls: Cell<usize>,
    }

    impl RuntimeModelCatalogSource for FakeCatalog {
        fn version(&self) -> Result<String, String> {
            self.version.borrow().clone()
        }

        fn discover(&self, _cwd: &Path) -> Result<RuntimeModelCatalog, String> {
            self.calls.set(self.calls.get() + 1);
            self.catalog.borrow().clone()
        }
    }

    fn descriptor() -> AiToolDescriptor {
        AiToolDescriptor {
            id: "example".into(),
            display_name: "Example".into(),
            models: vec![
                ModelSpec {
                    id: "stable".into(),
                    label: "Stable label".into(),
                    effort_levels: vec!["low".into()],
                },
                ModelSpec {
                    id: "fallback".into(),
                    label: "Fallback".into(),
                    effort_levels: vec!["medium".into()],
                },
            ],
            default_model: Some("fallback".into()),
            default_effort: Some("medium".into()),
            actions: vec!["change_model".into()],
        }
    }

    #[test]
    fn command_catalog_accepts_id_only_and_tab_labeled_runtime_rows() {
        let catalog = parse_command_catalog(
            "provider/first\nprovider/second\tSecond model\n\n",
            &["low".into(), "high".into()],
            Some("high"),
        );
        assert_eq!(
            catalog
                .models
                .iter()
                .map(|model| (model.id.as_str(), model.label.as_str()))
                .collect::<Vec<_>>(),
            [
                ("provider/first", "provider/first"),
                ("provider/second", "Second model"),
            ]
        );
        assert_eq!(catalog.models[0].effort_levels, ["low", "high"]);
        assert_eq!(catalog.models[0].default_effort.as_deref(), Some("high"));
    }

    fn catalog(model: &str, default: bool) -> RuntimeModelCatalog {
        RuntimeModelCatalog {
            models: vec![RuntimeModelSpec {
                id: model.into(),
                label: format!("Runtime {model}"),
                effort_levels: vec!["high".into()],
                default_effort: Some("high".into()),
                is_default: default,
            }],
        }
    }

    #[test]
    fn runtime_ids_and_efforts_merge_with_manifest_labels_and_defaults() {
        let source = FakeCatalog {
            version: RefCell::new(Ok("1".into())),
            catalog: RefCell::new(Ok(RuntimeModelCatalog {
                models: vec![
                    RuntimeModelSpec {
                        id: "stable".into(),
                        label: "Runtime Stable".into(),
                        effort_levels: vec!["medium".into(), "high".into()],
                        default_effort: Some("high".into()),
                        is_default: false,
                    },
                    RuntimeModelSpec {
                        id: "new".into(),
                        label: "New model".into(),
                        effort_levels: vec!["low".into()],
                        default_effort: Some("low".into()),
                        is_default: true,
                    },
                    RuntimeModelSpec {
                        id: "fallback".into(),
                        label: "Runtime Fallback".into(),
                        effort_levels: Vec::new(),
                        default_effort: None,
                        is_default: false,
                    },
                ],
            })),
            calls: Cell::new(0),
        };
        let resolved = ModelCatalogCache::default().resolve(
            descriptor(),
            Some(&source),
            Path::new("/work"),
            false,
        );
        assert_eq!(
            resolved
                .models
                .iter()
                .map(|model| {
                    (
                        model.id.clone(),
                        model.label.clone(),
                        model.effort_levels.clone(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (
                    "stable".to_string(),
                    "Stable label".to_string(),
                    vec!["medium".to_string(), "high".to_string()]
                ),
                (
                    "new".to_string(),
                    "New model".to_string(),
                    vec!["low".to_string()]
                ),
                (
                    "fallback".to_string(),
                    "Fallback".to_string(),
                    Vec::<String>::new()
                )
            ]
        );
        assert_eq!(resolved.default_model.as_deref(), Some("new"));
        assert_eq!(resolved.default_effort.as_deref(), Some("low"));
        assert_eq!(resolved.actions, ["change_model"]);
    }

    #[test]
    fn cache_reuses_a_version_and_invalidates_when_the_runtime_changes() {
        let source = FakeCatalog {
            version: RefCell::new(Ok("1".into())),
            catalog: RefCell::new(Ok(catalog("stable", true))),
            calls: Cell::new(0),
        };
        let mut cache = ModelCatalogCache::default();
        assert_eq!(
            cache
                .resolve(descriptor(), Some(&source), Path::new("/work"), false)
                .models[0]
                .id,
            "stable"
        );
        *source.catalog.borrow_mut() = Ok(catalog("new", true));
        assert_eq!(
            cache
                .resolve(descriptor(), Some(&source), Path::new("/work"), false)
                .models[0]
                .id,
            "stable",
            "same-version reads use the cache"
        );
        assert_eq!(source.calls.get(), 1);

        *source.version.borrow_mut() = Ok("2".into());
        assert_eq!(
            cache
                .resolve(descriptor(), Some(&source), Path::new("/work"), false)
                .models[0]
                .id,
            "new"
        );
        assert_eq!(source.calls.get(), 2);
    }

    #[test]
    fn errors_fall_back_without_retrying_until_forced_or_version_changes() {
        let source = FakeCatalog {
            version: RefCell::new(Ok("1".into())),
            catalog: RefCell::new(Err("offline".into())),
            calls: Cell::new(0),
        };
        let mut cache = ModelCatalogCache::default();
        for _ in 0..2 {
            assert_eq!(
                cache
                    .resolve(descriptor(), Some(&source), Path::new("/work"), false)
                    .models[0]
                    .id,
                "stable"
            );
        }
        assert_eq!(source.calls.get(), 1, "same-version failure is cached");
        *source.catalog.borrow_mut() = Ok(catalog("new", true));
        assert_eq!(
            cache
                .resolve(descriptor(), Some(&source), Path::new("/work"), true)
                .models[0]
                .id,
            "new",
            "an explicit client refresh retries"
        );
        assert_eq!(source.calls.get(), 2);

        *source.catalog.borrow_mut() = Err("transient refresh failure".into());
        assert_eq!(
            cache
                .resolve(descriptor(), Some(&source), Path::new("/work"), true)
                .models[0]
                .id,
            "new",
            "a failed forced refresh retains the same-version last-good catalog"
        );
        assert_eq!(source.calls.get(), 3);

        *source.version.borrow_mut() = Err("version unavailable".into());
        assert_eq!(
            cache
                .resolve(descriptor(), Some(&source), Path::new("/work"), true)
                .models[0]
                .id,
            "new",
            "a transient version-probe error retains the last good catalog"
        );
        assert_eq!(source.calls.get(), 3);
    }

    #[test]
    fn a_failed_new_runtime_version_does_not_reuse_the_old_catalog() {
        let source = FakeCatalog {
            version: RefCell::new(Ok("1".into())),
            catalog: RefCell::new(Ok(catalog("runtime-only", true))),
            calls: Cell::new(0),
        };
        let mut cache = ModelCatalogCache::default();
        assert_eq!(
            cache
                .resolve(descriptor(), Some(&source), Path::new("/work"), false)
                .models[0]
                .id,
            "runtime-only"
        );

        *source.version.borrow_mut() = Ok("2".into());
        *source.catalog.borrow_mut() = Err("upgraded runtime is unavailable".into());
        let fallback = cache.resolve(descriptor(), Some(&source), Path::new("/work"), false);
        assert_eq!(
            fallback
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["stable", "fallback"],
            "an old runtime catalog must not survive a version change"
        );
        cache.resolve(descriptor(), Some(&source), Path::new("/work"), false);
        assert_eq!(source.calls.get(), 2, "the new-version failure is cached");

        *source.version.borrow_mut() = Ok("3".into());
        *source.catalog.borrow_mut() = Ok(catalog("recovered", true));
        assert_eq!(
            cache
                .resolve(descriptor(), Some(&source), Path::new("/work"), false)
                .models[0]
                .id,
            "recovered"
        );
        assert_eq!(source.calls.get(), 3);
    }
}
