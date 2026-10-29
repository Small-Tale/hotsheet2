//! The notifier registry: built-ins first, then external plugins from
//! `${HOTSHEET_HOME}/notifiers/`. A built-in id always wins a collision.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::builtin::{email, ntfy, slack, speak, teams, twilio, webhook};
use crate::capability::NotifierDescriptor;
use crate::external;
use crate::notifier::{CommandRunner, LiveRunner, Notifier};

pub const NOTIFIERS_DIR: &str = "notifiers";

pub struct Registry {
    notifiers: Vec<Arc<dyn Notifier>>,
    /// Plugin directories that failed to load, with the reason.
    pub load_errors: Vec<String>,
}

/// Every first-party notifier over its live transport, in listing order.
fn live_builtins() -> Vec<Arc<dyn Notifier>> {
    vec![
        Arc::new(speak::Speak::live()),
        Arc::new(ntfy::Ntfy::live()),
        Arc::new(email::EmailNotifier::live()),
        Arc::new(webhook::Webhook::live()),
        Arc::new(slack::Slack::live()),
        Arc::new(teams::Teams::live()),
        Arc::new(twilio::Twilio::live()),
    ]
}

impl Registry {
    /// Live built-ins plus external plugins under `home/notifiers`.
    pub fn live(home: &Path) -> Registry {
        Registry::with_builtins(
            live_builtins(),
            Some(&Registry::plugins_dir(home)),
            Arc::new(LiveRunner),
        )
    }

    /// Live built-ins only (for validation where plugins are irrelevant).
    pub fn builtin_only() -> Registry {
        Registry::with_builtins(live_builtins(), None, Arc::new(LiveRunner))
    }

    /// The given notifiers plus external plugins from `plugins_dir`, run by `runner`.
    pub fn with_builtins(
        builtins: Vec<Arc<dyn Notifier>>,
        plugins_dir: Option<&Path>,
        runner: Arc<dyn CommandRunner>,
    ) -> Registry {
        let mut notifiers = builtins;
        let mut load_errors = Vec::new();
        if let Some(dir) = plugins_dir {
            let (loaded, errors) = external::load_all(dir, runner);
            load_errors.extend(errors);
            for plugin in loaded {
                let id = plugin.descriptor().id.clone();
                if notifiers.iter().any(|n| n.descriptor().id == id) {
                    load_errors.push(format!(
                        "notifier '{id}' is already provided; the plugin was ignored"
                    ));
                } else {
                    notifiers.push(Arc::new(plugin));
                }
            }
        }
        Registry {
            notifiers,
            load_errors,
        }
    }

    pub fn plugins_dir(home: &Path) -> PathBuf {
        home.join(NOTIFIERS_DIR)
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn Notifier>> {
        self.notifiers.iter().find(|n| n.descriptor().id == id)
    }

    pub fn descriptors(&self) -> Vec<NotifierDescriptor> {
        self.notifiers
            .iter()
            .map(|n| n.descriptor().clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingRunner;

    #[test]
    fn builtins_are_listed_and_win_id_collisions() {
        let home = tempfile::tempdir().unwrap();
        let dir = Registry::plugins_dir(home.path()).join("ntfy-clone");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            "id = \"ntfy\"\ndisplay_name = \"Fake\"\ncommand = [\"x\"]\n[capabilities]\nformats=[\"plain\"]\nmax_body_bytes = 10\n",
        )
        .unwrap();
        let registry = Registry::with_builtins(
            vec![Arc::new(ntfy::Ntfy::live()), Arc::new(speak::Speak::live())],
            Some(&Registry::plugins_dir(home.path())),
            Arc::new(RecordingRunner::succeeding()),
        );
        let ids: Vec<_> = registry.descriptors().into_iter().map(|d| d.id).collect();
        assert_eq!(ids, ["ntfy", "speak"]);
        assert_eq!(
            registry.get("ntfy").unwrap().descriptor().display_name,
            "ntfy push"
        );
        assert!(registry.load_errors[0].contains("already provided"));
        let live: Vec<_> = Registry::live(home.path())
            .descriptors()
            .into_iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(
            live,
            [
                "speak", "ntfy", "email", "webhook", "slack", "teams", "twilio"
            ]
        );
    }
}
