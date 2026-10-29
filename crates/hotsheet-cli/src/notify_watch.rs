//! Ticket-transition notifications for writes made when no server is running (HS2-8VAXXA,
//! `docs/32-notifier-plugins.md` §32.8). `hotsheet-cli` ticket commands and the serverless
//! `hotsheet-mcp` backend (HS2-FYZ5S9) both wrap their writes with a [`Watch`]. A running
//! server's store watcher already routes those writes, so this acts only when no live server
//! hosts the store; that is what keeps a transition from being sent twice. The messages come
//! from the same `hotsheet_notify::transitions` the server uses. A notification problem never
//! fails the write: it is reported on stderr and in the delivery log.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hotsheet_notify::transitions::{Origin, TicketFlags, TicketState, transition_messages};
use hotsheet_notify::{Dispatcher, EventKind, NotificationConfig, Outcome, Registry};
use hotsheet_ticketing::{FsStore, KeyRegistry, OsKeychain, TicketRow};

/// The index rows a transition compares, captured before a ticket write runs.
pub struct Watch {
    home: PathBuf,
    store: FsStore,
    index_path: PathBuf,
    before: HashMap<String, TicketFlags>,
}

fn wants_ticket_events(config: &NotificationConfig) -> bool {
    config.rules.iter().any(|r| {
        r.enabled
            && r.events.iter().any(|e| {
                matches!(
                    e,
                    EventKind::TicketStatusChanged
                        | EventKind::TicketFeedbackNeeded
                        | EventKind::TicketBlocked
                )
            })
    })
}

fn same_path(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canonical(a) == canonical(b)
}

#[cfg(unix)]
fn process_is_alive(pid: u64) -> bool {
    pid > 0
        && pid <= i32::MAX as u64
        && std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success())
}

/// Without a cheap liveness probe, a recorded server is assumed alive: skipping a send is
/// safer than sending twice.
#[cfg(not(unix))]
fn process_is_alive(_pid: u64) -> bool {
    true
}

/// Whether a live server registered itself as hosting `root` (`${HOTSHEET_HOME}/instances`).
pub fn server_hosts(home: &Path, root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(home.join("instances")) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            return false;
        };
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&text) else {
            return false;
        };
        let hosts = record
            .get("store_path")
            .and_then(|p| p.as_str())
            .is_some_and(|p| same_path(Path::new(p), root));
        hosts
            && record
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(process_is_alive)
    })
}

fn flags(row: &TicketRow) -> TicketFlags {
    TicketFlags {
        status: row.status.clone().unwrap_or_default(),
        feedback_needed: row.feedback_needed,
        blocked: row
            .blocked_reason
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty())
            || !row.blocked_by.is_empty(),
    }
}

fn rows(store: &FsStore, index_path: &Path) -> anyhow::Result<Vec<TicketRow>> {
    let index = hotsheet_index::Index::open_reconciled(index_path, store)?;
    Ok(index.query(&hotsheet_ticketing::ops::TicketQuery::default())?)
}

impl Watch {
    /// Start watching a write to the store at `store_path` when a rule wants ticket events
    /// and no live server hosts the store. `None` otherwise, or when the store cannot be read.
    pub fn before(store_path: &Path) -> Option<Watch> {
        let home = hotsheet_plugins::hotsheet_home();
        let config = NotificationConfig::load(&home).ok()?;
        if !wants_ticket_events(&config) {
            return None;
        }
        let store = FsStore::open(store_path).ok()?;
        if server_hosts(&home, store.root()) {
            return None;
        }
        let index_path = crate::default_index_path(&store).ok()?;
        let before = rows(&store, &index_path)
            .ok()?
            .iter()
            .map(|row| (row.id.clone(), flags(row)))
            .collect();
        Some(Watch {
            home,
            store,
            index_path,
            before,
        })
    }

    /// Diff the rows after the write and deliver each transition.
    pub fn after(self) {
        if let Err(error) = self.deliver() {
            eprintln!("warning: ticket notifications were not sent: {error}");
        }
    }

    fn deliver(self) -> anyhow::Result<()> {
        let config = NotificationConfig::load(&self.home)?;
        let after = rows(&self.store, &self.index_path)?;
        let store_id = hotsheet_ticketing::git_connection_id(&self.store);
        let root = self.store.root().to_path_buf();
        let linked: Vec<_> =
            hotsheet_ticketing::checkouts::CheckoutRegistry::new(self.home.join("checkouts.json"))
                .list()
                .unwrap_or_default()
                .into_iter()
                .filter(|c| {
                    same_path(Path::new(&c.root), &root)
                        || c.stores.iter().any(|s| same_path(Path::new(s), &root))
                })
                .collect();
        let project_name = linked
            .iter()
            .map(|c| c.alias.trim())
            .find(|a| !a.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                root.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.trim_end_matches(".hs2").to_string())
                    .unwrap_or_else(|| store_id.clone())
            });
        let origin = Origin {
            store: store_id,
            checkouts: linked.iter().map(|c| c.id.clone()).collect(),
            project_name,
        };
        let link_project = origin
            .checkouts
            .first()
            .cloned()
            .unwrap_or_else(|| root.display().to_string());
        let base = hotsheet_notify::links::link_base(&self.home, &config);
        let dispatcher = Dispatcher::new(
            Arc::new(Registry::live(&self.home)),
            Arc::new(KeyRegistry::new(&self.home, OsKeychain)),
            Some(&self.home),
        );
        let mut failed = Vec::new();
        for row in &after {
            let previous = self.before.get(&row.id);
            let current = TicketState {
                id: row.id.clone(),
                slug: row.slug.clone(),
                title: row.title.clone(),
                category: row.category.clone().unwrap_or_default(),
                priority: row.priority.clone().unwrap_or_default(),
                tags: row.tags.clone(),
                blocked_reason: row.blocked_reason.clone(),
                flags: flags(row),
            };
            if previous == Some(&current.flags) {
                continue;
            }
            for mut message in transition_messages(previous, &current, previous.is_none(), &origin)
            {
                if let Some(base) = &base {
                    message.url = Some(hotsheet_notify::links::deep_link(
                        base,
                        &link_project,
                        Some(&row.slug),
                    ));
                }
                for record in dispatcher.dispatch(&config, &message) {
                    if record.outcome == Outcome::Failed {
                        failed.push(format!(
                            "{}: {}",
                            record.channel,
                            record.error.unwrap_or_default()
                        ));
                    }
                }
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("{}", failed.join("; "))
        }
    }
}
