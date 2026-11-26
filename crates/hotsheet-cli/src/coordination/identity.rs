//! HS2-67N8SJ: a pool's `project_id` and `source_id` are derived from paths, so moving the
//! checkout or its ticket store changes them. The pool may follow the move only when the
//! machine's checkout registry proves both stored ids name this checkout and store:
//! the project by the durable registry id `relocate()` preserves, the source by following
//! the checkout's retained `sourceAliases` to a store it links today. Anything else is
//! refused, and nothing ever force-rebinds a pool.

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::{Result, bail, ensure};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use serde::Serialize;

use super::{Coordinator, State};

/// Appended once per migration, beside `state.json`; the pool schema is unchanged.
pub(super) const MIGRATION_LOG: &str = "identity-migrations.log";

/// Length of a Git connection id: the hex prefix of sha256(canonical store path).
const CONNECTION_ID_LEN: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStatus {
    /// No pool exists yet; the next open creates it with the current ids.
    Absent,
    /// The stored ids already equal the recomputed ones.
    Current,
    /// The registry proves the stored ids name this checkout and store.
    Equivalent,
    /// No proof; the pool is left unchanged.
    Refused,
}

/// What `coordination repair-identity` reports; also the basis of the automatic migration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IdentityReport {
    pub status: IdentityStatus,
    pub stored_project_id: Option<String>,
    pub stored_source_id: Option<String>,
    pub current_project_id: String,
    pub current_source_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
    pub dry_run: bool,
    /// True only when this call rewrote the pool's ids.
    pub migrated: bool,
}

enum Proof {
    Current,
    Equivalent { project: String, source: String },
    Refused(String),
}

/// The exact headless recovery command named by every identity refusal.
pub(super) fn repair_command(project: &Path, source: &Path) -> String {
    format!(
        "hotsheet-cli -C '{}' coordination --project '{}' repair-identity --dry-run",
        source.display(),
        project.display()
    )
}

impl Coordinator {
    /// HS2-67N8SJ: report, and unless `dry_run` migrate, a pool whose ids predate a proven
    /// checkout or store move. Never creates a pool and never rebinds an unproven one.
    pub fn repair_identity(
        project: &Path,
        source: &Path,
        registry_home: std::path::PathBuf,
        dry_run: bool,
    ) -> Result<IdentityReport> {
        let this = Self::locate(project, source, registry_home)?;
        let _lock = this.lock()?;
        let path = this.directory.join("state.json");
        let mut report = IdentityReport {
            status: IdentityStatus::Absent,
            stored_project_id: None,
            stored_source_id: None,
            current_project_id: this.project_id.clone(),
            current_source_id: this.source_id.clone(),
            project_evidence: None,
            source_evidence: None,
            refusal: None,
            dry_run,
            migrated: false,
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
            Err(error) => return Err(error.into()),
        };
        let mut state = serde_json::from_slice::<State>(&bytes)?;
        ensure!(
            matches!(state.version, 1..=9),
            "incompatible coordination state: pool schema {} is not supported by this executable",
            state.version
        );
        report.stored_project_id = Some(state.project_id.clone());
        report.stored_source_id = Some(state.source_id.clone());
        match this.prove(&state) {
            Proof::Current => report.status = IdentityStatus::Current,
            Proof::Refused(reason) => {
                report.status = IdentityStatus::Refused;
                report.refusal = Some(reason);
            }
            Proof::Equivalent {
                project: project_evidence,
                source: source_evidence,
            } => {
                report.status = IdentityStatus::Equivalent;
                if !dry_run {
                    let line =
                        this.migrate(&mut state, &project_evidence, &source_evidence, "repair");
                    state.revision += 1;
                    this.write_state(&state, &path)?;
                    this.record_migration(&line)?;
                    report.migrated = true;
                }
                report.project_evidence = Some(project_evidence);
                report.source_evidence = Some(source_evidence);
            }
        }
        Ok(report)
    }

    /// Inside `change()`, under the state lock: follow a proven move, or refuse naming the
    /// repair command. A pool that already matches is untouched. Returns the provenance
    /// line to record once the migrated state is persisted.
    pub(super) fn reconcile_identity(&self, state: &mut State) -> Result<Option<String>> {
        match self.prove(state) {
            Proof::Current => Ok(None),
            Proof::Equivalent { project, source } => {
                Ok(Some(self.migrate(state, &project, &source, "automatic")))
            }
            Proof::Refused(reason) => {
                let hint = repair_command(&self.project, &self.source);
                if state.project_id == self.project_id {
                    bail!(
                        "coordination pool is bound to another source; use the same explicit -C source as the integrator ({reason}). If this checkout or its ticket store moved, run `{hint}`"
                    )
                }
                bail!(
                    "incompatible coordination state: {reason}. If this checkout or its ticket store moved, run `{hint}`"
                )
            }
        }
    }

    /// Append one provenance line after the migrated pool was persisted.
    pub(super) fn record_migration(&self, line: &str) -> Result<()> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.join(MIGRATION_LOG))?
            .write_all(line.as_bytes())?;
        Ok(())
    }

    /// Rewrite only the two ids, keeping every other field; returns the provenance line.
    fn migrate(
        &self,
        state: &mut State,
        project_evidence: &str,
        source_evidence: &str,
        via: &str,
    ) -> String {
        let line = format!(
            "{} HS2-67N8SJ {via} identity migration: project_id {} -> {} ({project_evidence}); source_id {} -> {} ({source_evidence})\n",
            time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            state.project_id,
            self.project_id,
            state.source_id,
            self.source_id,
        );
        state.project_id.clone_from(&self.project_id);
        state.source_id.clone_from(&self.source_id);
        line
    }

    fn prove(&self, state: &State) -> Proof {
        if state.project_id == self.project_id && state.source_id == self.source_id {
            return Proof::Current;
        }
        let registry = CheckoutRegistry::new(self.registry_home.join("checkouts.json"));
        let entry = match registry.checkout_identity_at_root(&self.project) {
            Ok(entry) => entry,
            Err(error) => {
                return Proof::Refused(format!("the checkout registry is unreadable: {error}"));
            }
        };
        let project = if state.project_id == self.project_id {
            "current checkout id".to_owned()
        } else {
            match &entry {
                Some((checkout, _)) if checkout.id == state.project_id => {
                    "durable checkout registry id of the checkout registered at this root"
                        .to_owned()
                }
                Some((checkout, _)) => {
                    return Proof::Refused(format!(
                        "stored project id {} is neither this checkout's id {} nor the registry id {} of the checkout registered here",
                        state.project_id, self.project_id, checkout.id
                    ));
                }
                None => {
                    return Proof::Refused(format!(
                        "stored project id {} is not this checkout's id {} and no checkout is registered at this root",
                        state.project_id, self.project_id
                    ));
                }
            }
        };
        if state.source_id == self.source_id {
            return Proof::Equivalent {
                project,
                source: "current source id".to_owned(),
            };
        }
        let Some((checkout, aliases)) = entry else {
            return Proof::Refused(
                "stored source id differs and no checkout registered at this root records source aliases".to_owned(),
            );
        };
        if state.source_id.len() != 64 || !state.source_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Proof::Refused("stored source id is not a store path hash".to_owned());
        }
        let canonical = self.source.to_string_lossy();
        let current: BTreeSet<&str> = checkout
            .sources
            .iter()
            .filter(|source| {
                source.provider == "git"
                    && hotsheet_sync::paths::canonicalize(Path::new(&source.locator))
                        .map_or(source.locator == canonical, |path| {
                            path.to_string_lossy() == canonical
                        })
            })
            .map(|source| source.connection_id.as_str())
            .collect();
        if current.is_empty() {
            return Proof::Refused(format!(
                "checkout {} does not link the current ticket store",
                checkout.id
            ));
        }
        let start = &state.source_id[..CONNECTION_ID_LEN];
        if current.contains(start) {
            // Same connection id but a different full hash: not a move this proof explains.
            return Proof::Refused(format!(
                "stored source id shares connection id {start} with the current store but differs from its hash"
            ));
        }
        let mut chain = vec![start];
        let mut visited = BTreeSet::from([start]);
        let mut at = start;
        loop {
            let Some(next) = aliases.get(at).map(String::as_str) else {
                return Proof::Refused(format!(
                    "source alias chain {} for checkout {} does not reach the current store's connection id",
                    chain.join(" -> "),
                    checkout.id
                ));
            };
            chain.push(next);
            if current.contains(next) {
                return Proof::Equivalent {
                    project,
                    source: format!("source alias chain {}", chain.join(" -> ")),
                };
            }
            if !visited.insert(next) {
                return Proof::Refused(format!(
                    "source alias cycle {} in checkout {}",
                    chain.join(" -> "),
                    checkout.id
                ));
            }
            at = next;
        }
    }
}
