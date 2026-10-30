//! `hotsheet-cli notify`: headless notifier plugin setup and sending (HS2-R5HTZN,
//! `docs/32-notifier-plugins.md` §32.9). Every subcommand runs the same
//! `hotsheet_notify::admin` workflow the server routes run, against the machine home, and
//! needs no running server.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use hotsheet_notify::admin::{self, AdminError};
use hotsheet_notify::{
    Attachment, ChannelConfig, DeliveryRecord, Dispatcher, EventKind, Message, NotificationConfig,
    Outcome, Registry, Rule, RuleFilter, SettingKind, Severity, ViewRef, recent_deliveries,
};
use hotsheet_ticketing::{KeyRegistry, OsKeychain};
use serde_json::{Value, json};

use super::{Ctx, misrouted};
use crate::Cmd;

#[derive(Subcommand)]
pub(crate) enum NotifyCmd {
    /// List installed notifier plugins with their capabilities and settings.
    Notifiers {
        #[arg(long)]
        json: bool,
    },
    /// Show configured channels and rules (credentials appear only as key names).
    Config {
        #[arg(long)]
        json: bool,
    },
    /// Configure delivery channels.
    Channel {
        #[command(subcommand)]
        cmd: ChannelCmd,
    },
    /// Configure which events go to which channels.
    Rule {
        #[command(subcommand)]
        cmd: RuleCmd,
    },
    /// Send a notification now: to one channel, or through the rules when no channel is given.
    Send {
        #[arg(long)]
        title: String,
        /// Markdown body.
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read the Markdown body from a file (`-` for stdin).
        #[arg(long)]
        body_file: Option<PathBuf>,
        /// Deliver to this channel only, ignoring rules.
        #[arg(long)]
        channel: Option<String>,
        /// The event kind rules match (default `notification`).
        #[arg(long, default_value = "notification")]
        event: String,
        /// info | success | warning | error.
        #[arg(long, default_value = "info")]
        severity: String,
        /// Project (checkout id or store id) for rule filters.
        #[arg(long)]
        project: Option<String>,
        /// Display name for the project.
        #[arg(long)]
        project_name: Option<String>,
        /// Click-through URL.
        #[arg(long)]
        url: Option<String>,
        /// Attach a file (repeatable); channels that cannot carry it say it was omitted.
        #[arg(long = "attach")]
        attachments: Vec<PathBuf>,
        /// Repeats with this key inside a rule's cooldown are suppressed.
        #[arg(long)]
        dedupe_key: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Set the client origin click-through links open (a LAN or public origin for phones);
    /// with no URL, show it. Unset, links use the local client's origin.
    LinkBase {
        url: Option<String>,
        /// Clear the configured origin.
        #[arg(long, conflicts_with = "url")]
        clear: bool,
    },
    /// Show or set project.stopped timing: the release grace and the idle window while
    /// claims are held (0 disables idle detection).
    ProjectTiming {
        #[arg(long)]
        grace_seconds: Option<u64>,
        #[arg(long)]
        idle_minutes: Option<u64>,
    },
    /// Show recent deliveries (ids and outcomes only).
    Deliveries {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum ChannelCmd {
    /// Create or replace a channel. Idempotent; stored credentials are kept.
    Add {
        id: String,
        /// Notifier plugin id (see `notify notifiers`).
        #[arg(long)]
        notifier: String,
        #[arg(long)]
        label: Option<String>,
        /// A setting as key=value (repeatable). Lists are comma-separated.
        #[arg(long = "set", value_name = "KEY=VALUE")]
        settings: Vec<String>,
        /// Create the channel disabled.
        #[arg(long)]
        disabled: bool,
        /// Hold rule deliveries during this daily window, HH:MM-HH:MM (may wrap midnight).
        #[arg(long, value_name = "HH:MM-HH:MM")]
        quiet: Option<String>,
        /// UTC offset of the quiet window, like +09:00 (default +00:00).
        #[arg(long, value_name = "+HH:MM", allow_hyphen_values = true)]
        quiet_offset: Option<String>,
        /// Hold error-severity messages during quiet hours too.
        #[arg(long)]
        quiet_errors: bool,
        /// Deliver at most this many rule notifications per rolling hour.
        #[arg(long)]
        max_per_hour: Option<u32>,
    },
    /// Store a credential for a channel's secret setting. Prompts on a terminal or reads
    /// piped stdin; the value goes to the machine key store, never to the config file.
    Secret { id: String, field: String },
    /// Enable a channel so matching rules deliver to it.
    Enable { id: String },
    /// Disable a channel; rules skip it, but `channel test` still sends.
    Disable { id: String },
    /// Remove a channel, rules left without a channel, and its stored credentials.
    Remove { id: String },
    /// Approve the channel's external notifier plugin as it is now. Delivery refuses a
    /// plugin that changed since its channel approved it.
    Trust { id: String },
    /// Send a test notification to the channel, even when it is disabled.
    Test {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum RuleCmd {
    /// Create or replace a rule. Idempotent.
    Add(Box<RuleAddArgs>),
    /// Remove a rule.
    Remove { id: String },
}

#[derive(clap::Args)]
pub(crate) struct RuleAddArgs {
    pub(crate) id: String,
    /// Event kind (repeatable): ticket.status_changed, ticket.feedback_needed,
    /// ticket.blocked, permission.requested, notification, digest, project.stopped.
    #[arg(long = "event", required = true)]
    pub(crate) events: Vec<String>,
    /// Channel id (repeatable).
    #[arg(long = "channel", required = true)]
    pub(crate) channels: Vec<String>,
    /// Only events from this project (checkout id, store id, or path); repeatable.
    #[arg(long = "project")]
    pub(crate) projects: Vec<String>,
    /// Only tickets carrying one of these tags; repeatable.
    #[arg(long = "tag")]
    pub(crate) tags: Vec<String>,
    /// Only tickets whose (new) status is one of these; repeatable.
    #[arg(long = "status")]
    pub(crate) statuses: Vec<String>,
    #[arg(long = "category")]
    pub(crate) categories: Vec<String>,
    #[arg(long = "priority")]
    pub(crate) priorities: Vec<String>,
    /// Minimum severity: info | success | warning | error.
    #[arg(long)]
    pub(crate) min_severity: Option<String>,
    /// Only tickets whose slug, title, or a tag contains this text (case-insensitive).
    #[arg(long)]
    pub(crate) text: Option<String>,
    /// Only tickets in this saved view, as CHECKOUT/VIEW (checkout id, alias, or path, then the
    /// view id); repeatable, any one matches.
    #[arg(long = "view", value_name = "CHECKOUT/VIEW")]
    pub(crate) views: Vec<String>,
    /// Suppress repeats of the same event per channel for this many seconds.
    #[arg(long, default_value_t = 0)]
    pub(crate) cooldown: u64,
    #[arg(long)]
    pub(crate) disabled: bool,
}

/// Resolve `CHECKOUT/VIEW` to a registered checkout's id and one of its saved views. The view
/// id is the part after the last `/`, so a checkout path may contain slashes.
fn view_ref(home: &Path, reference: &str) -> Result<ViewRef> {
    let (checkout, view) = reference
        .rsplit_once('/')
        .filter(|(c, v)| !c.trim().is_empty() && !v.trim().is_empty())
        .ok_or_else(|| anyhow!("--view expects CHECKOUT/VIEW, got '{reference}'"))?;
    let checkout =
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"))
            .resolve(checkout.trim())
            .map_err(|e| anyhow!("--view {reference}: {e}"))?;
    let views = hotsheet_ticketing::saved_views::from_settings(&checkout.settings())
        .map_err(|e| anyhow!("--view {reference}: {e}"))?;
    let saved = views
        .iter()
        .find(|v| v.id.eq_ignore_ascii_case(view.trim()))
        .ok_or_else(|| {
            let known: Vec<_> = views.iter().map(|v| v.id.as_str()).collect();
            anyhow!(
                "checkout {} has no saved view '{}' (saved views: {})",
                checkout.id,
                view.trim(),
                if known.is_empty() {
                    "none".into()
                } else {
                    known.join(", ")
                }
            )
        })?;
    Ok(ViewRef {
        checkout: checkout.id,
        view: saved.id.clone(),
    })
}

fn home() -> PathBuf {
    hotsheet_plugins::hotsheet_home()
}

fn keys(home: &Path) -> KeyRegistry<OsKeychain> {
    KeyRegistry::new(home, OsKeychain)
}

fn admin_err(error: AdminError) -> anyhow::Error {
    anyhow!(error.to_string())
}

pub(crate) fn run(cmd: Cmd, _ctx: &Ctx) -> Result<()> {
    let Cmd::Notify { cmd } = cmd else {
        misrouted("notify")
    };
    let home = home();
    let registry = Registry::live(&home);
    for error in &registry.load_errors {
        eprintln!("warning: {error}");
    }
    match cmd {
        NotifyCmd::Notifiers { json } => {
            let descriptors = registry.descriptors();
            if json {
                println!("{}", serde_json::to_string_pretty(&descriptors)?);
                return Ok(());
            }
            for d in descriptors {
                let formats: Vec<_> = d
                    .capabilities
                    .formats
                    .iter()
                    .map(|f| json!(f).as_str().unwrap_or_default().to_string())
                    .collect();
                println!("{} — {}", d.id, d.display_name);
                println!(
                    "  formats: {}; body ≤ {} bytes; attachments: {}",
                    formats.join(", "),
                    d.capabilities.max_body_bytes,
                    d.capabilities.attachments.max_count
                );
                for field in d.settings {
                    let required = if field.required { " (required)" } else { "" };
                    println!("  --set {}=…  {}{required}", field.key, field.label);
                }
            }
        }
        NotifyCmd::Config { json } => {
            let config = NotificationConfig::load(&home)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&config)?);
                return Ok(());
            }
            if config.channels.is_empty() {
                println!("(no channels; add one with `hotsheet-cli notify channel add`)");
            }
            let trust = admin::trust_status(&config, &registry);
            for c in &config.channels {
                let state = if c.enabled { "" } else { " [disabled]" };
                let changed = trust
                    .iter()
                    .any(|(id, status)| id == &c.id && *status == Some(false));
                let changed = if changed {
                    format!(
                        " [plugin changed: review it, then `hotsheet-cli notify channel trust {}`]",
                        c.id
                    )
                } else {
                    String::new()
                };
                println!("channel {} ({}){state}{changed}", c.id, c.notifier);
            }
            for r in &config.rules {
                let events: Vec<_> = r.events.iter().map(|e| e.as_str()).collect();
                let state = if r.enabled { "" } else { " [disabled]" };
                println!(
                    "rule {}: {} → {}{state}",
                    r.id,
                    events.join(", "),
                    r.channels.join(", ")
                );
            }
        }
        NotifyCmd::Channel { cmd } => channel(&home, &registry, cmd)?,
        NotifyCmd::Rule { cmd } => rule(&home, &registry, cmd)?,
        NotifyCmd::Send {
            title,
            body,
            body_file,
            channel,
            event,
            severity,
            project,
            project_name,
            url,
            attachments,
            dedupe_key,
            json,
        } => {
            let event =
                EventKind::parse(&event).ok_or_else(|| anyhow!("unknown event '{event}'"))?;
            let mut message = Message::new(event, title);
            message.body = match body_file {
                Some(path) if path.as_os_str() == "-" => std::io::read_to_string(std::io::stdin())?,
                Some(path) => std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?,
                None => body.unwrap_or_default(),
            };
            message.severity = parse_severity(&severity)?;
            message.project = project;
            message.project_name = project_name;
            message.url = url;
            message.dedupe_key = dedupe_key;
            message.attachments = attachments
                .iter()
                .map(|path| read_attachment(path))
                .collect::<Result<_>>()?;
            let config = NotificationConfig::load(&home)?;
            let dispatcher = dispatcher(&home, registry);
            let records = match channel {
                Some(channel) => vec![
                    dispatcher
                        .deliver_to(&config, &channel, &message)
                        .map_err(|e| anyhow!(e))?,
                ],
                None => {
                    if event == EventKind::Test {
                        bail!("a test notification needs --channel");
                    }
                    dispatcher.dispatch(&config, &message)
                }
            };
            report(&records, json)?;
        }
        NotifyCmd::LinkBase { url, clear } => {
            if clear || url.is_some() {
                admin::set_link_base(&home, &registry, url).map_err(admin_err)?;
            }
            let config = NotificationConfig::load(&home)?;
            match (
                &config.link_base,
                hotsheet_notify::links::link_base(&home, &config),
            ) {
                (Some(base), _) => println!("Links open {base}"),
                (None, Some(local)) => {
                    println!("Links open the local client at {local} (no link base set)")
                }
                (None, None) => println!(
                    "No link base set and no local client recorded; notifications carry no links"
                ),
            }
        }
        NotifyCmd::ProjectTiming {
            grace_seconds,
            idle_minutes,
        } => {
            let timing = if grace_seconds.is_some() || idle_minutes.is_some() {
                admin::set_project_timing(&home, &registry, grace_seconds, idle_minutes)
                    .map_err(admin_err)?
            } else {
                NotificationConfig::load(&home)?
                    .project_activity
                    .unwrap_or_default()
            };
            let grace = timing.grace_seconds.unwrap_or(120);
            let idle = match timing.idle_minutes.unwrap_or(30) {
                0 => "off".to_string(),
                minutes => format!("{minutes} min"),
            };
            println!("project.stopped grace: {grace} s; idle while claimed: {idle}");
        }
        NotifyCmd::Deliveries { limit, json } => {
            let records = recent_deliveries(&home, limit.clamp(1, 500));
            if json {
                println!("{}", serde_json::to_string_pretty(&records)?);
            } else {
                for r in records {
                    println!(
                        "{} {} {} → {} {:?} ({} attempt(s)){}",
                        r.at,
                        r.event.as_str(),
                        r.ticket.as_deref().unwrap_or("-"),
                        r.channel,
                        r.outcome,
                        r.attempts,
                        r.error.map(|e| format!(": {e}")).unwrap_or_default()
                    );
                }
            }
        }
    }
    Ok(())
}

fn dispatcher(home: &Path, registry: Registry) -> Dispatcher {
    Dispatcher::new(Arc::new(registry), Arc::new(keys(home)), Some(home))
}

fn report(records: &[DeliveryRecord], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(records)?);
    } else if records.is_empty() {
        println!("No enabled rule matched; nothing was sent.");
    } else {
        for r in records {
            match r.outcome {
                Outcome::Delivered => println!("delivered to {} ({})", r.channel, r.notifier),
                Outcome::Suppressed => {
                    println!("suppressed for {} (inside the rule's cooldown)", r.channel)
                }
                Outcome::Failed => println!(
                    "failed for {} after {} attempt(s): {}",
                    r.channel,
                    r.attempts,
                    r.error.as_deref().unwrap_or("unknown error")
                ),
            }
        }
    }
    if records.iter().any(|r| r.outcome == Outcome::Failed) {
        bail!("delivery failed");
    }
    Ok(())
}

fn channel(home: &Path, registry: &Registry, cmd: ChannelCmd) -> Result<()> {
    match cmd {
        ChannelCmd::Add {
            id,
            notifier,
            label,
            settings,
            disabled,
            quiet,
            quiet_offset,
            quiet_errors,
            max_per_hour,
        } => {
            let quiet_hours = quiet
                .map(|window| {
                    let (start, end) = window
                        .split_once('-')
                        .ok_or_else(|| anyhow!("--quiet expects HH:MM-HH:MM, got '{window}'"))?;
                    Ok::<_, anyhow::Error>(hotsheet_notify::config::QuietHours {
                        start: start.trim().to_string(),
                        end: end.trim().to_string(),
                        utc_offset: quiet_offset.clone().unwrap_or_else(|| "+00:00".into()),
                        allow_errors: !quiet_errors,
                    })
                })
                .transpose()?;
            let descriptor = registry
                .get(&notifier)
                .ok_or_else(|| {
                    anyhow!("unknown notifier '{notifier}' (see `hotsheet-cli notify notifiers`)")
                })?
                .descriptor()
                .clone();
            let mut parsed = hotsheet_notify::settings::Settings::new();
            for pair in settings {
                let (key, value) = pair
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--set expects KEY=VALUE, got '{pair}'"))?;
                let field = descriptor
                    .settings
                    .iter()
                    .find(|f| f.key == key)
                    .ok_or_else(|| anyhow!("{notifier} has no setting '{key}'"))?;
                parsed.insert(
                    key.to_string(),
                    setting_value(&field.kind, key, value, &id)?,
                );
            }
            let saved = admin::put_channel(
                home,
                registry,
                ChannelConfig {
                    id: id.clone(),
                    notifier,
                    label,
                    enabled: !disabled,
                    settings: parsed,
                    plugin_digest: None,
                    quiet_hours,
                    max_per_hour,
                },
            )
            .map_err(admin_err)?;
            println!("Saved channel {}", saved.id);
            let missing: Vec<_> = descriptor
                .settings
                .iter()
                .filter(|f| f.kind == SettingKind::Secret && !saved.settings.contains_key(&f.key))
                .map(|f| f.key.as_str())
                .collect();
            for field in missing {
                let need = descriptor
                    .settings
                    .iter()
                    .any(|f| f.key == field && f.required);
                let kind = if need { "required" } else { "optional" };
                println!("  {kind} credential: hotsheet-cli notify channel secret {id} {field}");
            }
        }
        ChannelCmd::Secret { id, field } => {
            let stdin = std::io::stdin();
            let value = super::providers::read_key_secret(
                &format!("{id} {field}"),
                stdin.is_terminal(),
                stdin.lock(),
                |prompt| rpassword::prompt_password(prompt).map_err(Into::into),
            )?;
            admin::set_channel_secret(home, registry, &keys(home), &id, &field, &value)
                .map_err(admin_err)?;
            println!("Stored the {field} credential for {id} in the machine key store");
        }
        ChannelCmd::Enable { id } => {
            admin::set_channel_enabled(home, registry, &id, true).map_err(admin_err)?;
            println!("Enabled channel {id}");
        }
        ChannelCmd::Disable { id } => {
            admin::set_channel_enabled(home, registry, &id, false).map_err(admin_err)?;
            println!("Disabled channel {id}");
        }
        ChannelCmd::Remove { id } => {
            let removal =
                admin::remove_channel(home, registry, &keys(home), &id).map_err(admin_err)?;
            if !removal.removed {
                println!("No channel {id}");
                return Ok(());
            }
            println!("Removed channel {id}");
            for rule in removal.removed_rules {
                println!("  removed rule {rule} (it had no other channel)");
            }
            for secret in removal.deleted_secrets {
                println!("  deleted credential {secret}");
            }
        }
        ChannelCmd::Trust { id } => {
            let trusted = admin::trust_channel(home, registry, &id).map_err(admin_err)?;
            match trusted.plugin_digest {
                Some(digest) => println!(
                    "Approved notifier plugin {} for channel {id} (digest {})",
                    trusted.notifier,
                    &digest[..12]
                ),
                None => println!(
                    "Channel {id} uses built-in notifier {}; nothing to approve",
                    trusted.notifier
                ),
            }
        }
        ChannelCmd::Test { id, json } => {
            let dispatcher = dispatcher(home, Registry::live(home));
            let record = admin::test_channel(home, &dispatcher, &id).map_err(admin_err)?;
            report(&[record], json)?;
        }
    }
    Ok(())
}

/// Parse a `--set` value by the field's declared kind.
fn setting_value(kind: &SettingKind, key: &str, value: &str, channel: &str) -> Result<Value> {
    Ok(match kind {
        SettingKind::String | SettingKind::Choice { .. } => Value::String(value.to_string()),
        SettingKind::List => Value::Array(
            value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| Value::String(v.to_string()))
                .collect(),
        ),
        SettingKind::Number => match value.parse::<i64>() {
            Ok(integer) => json!(integer),
            Err(_) => json!(
                value
                    .parse::<f64>()
                    .map_err(|_| anyhow!("setting '{key}' must be a number"))?
            ),
        },
        SettingKind::Bool => json!(
            value
                .parse::<bool>()
                .map_err(|_| anyhow!("setting '{key}' must be true or false"))?
        ),
        SettingKind::Secret => bail!(
            "'{key}' is a credential; store it with `hotsheet-cli notify channel secret {channel} {key}`"
        ),
    })
}

fn parse_severity(value: &str) -> Result<Severity> {
    serde_json::from_value(json!(value))
        .map_err(|_| anyhow!("severity must be info, success, warning, or error"))
}

fn read_attachment(path: &Path) -> Result<Attachment> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let content_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "json" => "application/json",
        _ => "application/octet-stream",
    };
    Ok(Attachment {
        name,
        content_type: content_type.into(),
        bytes,
    })
}

fn rule(home: &Path, registry: &Registry, cmd: RuleCmd) -> Result<()> {
    match cmd {
        RuleCmd::Add(args) => {
            let RuleAddArgs {
                id,
                events,
                channels,
                projects,
                tags,
                statuses,
                categories,
                priorities,
                min_severity,
                text,
                views,
                cooldown,
                disabled,
            } = *args;
            let views = views
                .iter()
                .map(|reference| view_ref(home, reference))
                .collect::<Result<Vec<_>>>()?;
            let events = events
                .iter()
                .map(|e| EventKind::parse(e).ok_or_else(|| anyhow!("unknown event '{e}'")))
                .collect::<Result<Vec<_>>>()?;
            let rule = Rule {
                id,
                events,
                channels,
                enabled: !disabled,
                filter: RuleFilter {
                    projects,
                    tags,
                    statuses,
                    categories,
                    priorities,
                    min_severity: min_severity.as_deref().map(parse_severity).transpose()?,
                    text,
                    views,
                },
                cooldown_seconds: cooldown,
            };
            let saved = admin::put_rule(home, registry, rule).map_err(admin_err)?;
            println!("Saved rule {}", saved.id);
        }
        RuleCmd::Remove { id } => {
            if admin::remove_rule(home, registry, &id).map_err(admin_err)? {
                println!("Removed rule {id}");
            } else {
                println!("No rule {id}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_values_parse_by_field_kind() {
        assert_eq!(
            setting_value(&SettingKind::List, "to", "a@x.test, b@x.test,", "c").unwrap(),
            json!(["a@x.test", "b@x.test"])
        );
        assert_eq!(
            setting_value(&SettingKind::Number, "p", "587", "c").unwrap(),
            json!(587)
        );
        assert_eq!(
            setting_value(&SettingKind::Bool, "b", "true", "c").unwrap(),
            json!(true)
        );
        assert!(setting_value(&SettingKind::Number, "p", "x", "c").is_err());
        let err = setting_value(&SettingKind::Secret, "token", "v", "phone")
            .unwrap_err()
            .to_string();
        assert!(err.contains("notify channel secret phone token"), "{err}");
        assert_eq!(parse_severity("warning").unwrap(), Severity::Warning);
        assert!(parse_severity("loud").is_err());
    }
}
