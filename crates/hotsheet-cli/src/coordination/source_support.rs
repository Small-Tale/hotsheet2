//! Metadata-only source diagnostics. Never resolve providers, credentials or tickets.

use std::collections::BTreeMap;
use std::path::Path;

use hotsheet_ticketing::{ProviderCapabilities, ProviderConnection, checkouts::Checkout};

use super::contract::{CoverageGap, CoverageGapReason, SourceConfiguration, SourceSupport};

impl super::Coordinator {
    /// Explain an existing foreign-source rejection without resolving its tickets or secrets.
    pub fn unsupported_source(&self, connection_id: &str) -> anyhow::Result<String> {
        let connections =
            hotsheet_ticketing::ProviderConfigRegistry::new(self.source.join("providers.json"))
                .load()?;
        let checkouts: Vec<_> = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
            hotsheet_plugins::hotsheet_home().join("checkouts.json"),
        )
        .list()?
        .into_iter()
        .filter(|c| c.id == self.project_id)
        .collect();
        let reports = reports(None, &self.source, &connections, &checkouts);
        Ok(
            match reports.iter().find(|r| r.connection_id == connection_id) {
                Some(report) => format!(
                    "source '{connection_id}' ({}): {}; provider claim/handoff verification is unsupported (HS2-3ETJ26)",
                    report.provider,
                    report.reasons.join(", "),
                ),
                None => format!(
                    "source '{connection_id}' is unknown to the configured source metadata; provider claim/handoff verification is unsupported (HS2-3ETJ26)"
                ),
            },
        )
    }
}

fn describe(connection: &ProviderConnection) -> SourceSupport {
    let descriptor = hotsheet_extsync::descriptor(connection);
    let mut report = SourceSupport {
        connection_id: connection.id.clone(),
        provider: connection.provider.clone(),
        configuration: SourceConfiguration::Ready,
        capabilities: descriptor.ok().map(|d| d.capabilities),
        native_query: false,
        evidence_read: false,
        queue_observed: false,
        claim_verification: false,
        event_subscription: false,
        reasons: Vec::new(),
    };
    if let Some(capabilities) = &report.capabilities {
        report.native_query = true;
        report.evidence_read = capabilities.notes;
        if !capabilities.up_next {
            report.reasons.push("up_next_unsupported".into());
        }
        if !capabilities.claims {
            report.reasons.push("native_claims_unsupported".into());
        }
        report
            .reasons
            .push("coordination_events_unavailable".into());
    } else {
        report.configuration = SourceConfiguration::Invalid;
        report
            .reasons
            .push("provider_descriptor_unavailable".into());
    }
    if connection.disabled {
        report.configuration = SourceConfiguration::Disabled;
        report.reasons.insert(0, "source_disabled".into());
    }
    report
}

fn local(connection_id: String, selected: bool) -> SourceSupport {
    SourceSupport {
        connection_id,
        provider: "git".into(),
        configuration: SourceConfiguration::Ready,
        capabilities: Some(ProviderCapabilities::git()),
        native_query: true,
        evidence_read: true,
        queue_observed: selected,
        claim_verification: selected,
        event_subscription: selected,
        reasons: if selected {
            Vec::new()
        } else {
            vec!["additional_local_source_not_observed".into()]
        },
    }
}

/// Reuse the exact enumeration inputs used for uncovered-source fencing. No store is
/// opened here; selected-source facts already came from the existing local snapshot.
pub(super) fn reports(
    selected: Option<&str>,
    source_path: &Path,
    connections: &[ProviderConnection],
    checkouts: &[Checkout],
) -> Vec<SourceSupport> {
    let mut reports: BTreeMap<String, SourceSupport> = connections
        .iter()
        .map(|c| (c.id.clone(), describe(c)))
        .collect();
    for checkout in checkouts {
        for source in &checkout.sources {
            if let Some(report) = reports.get_mut(&source.connection_id) {
                if report.configuration != SourceConfiguration::Disabled
                    && connections.iter().any(|c| {
                        c.id == source.connection_id
                            && (c.provider != source.provider || c.locator != source.locator)
                    })
                {
                    report.configuration = SourceConfiguration::Reconfigured;
                    report
                        .reasons
                        .push("checkout_source_metadata_mismatch".into());
                }
            } else if source.provider == "git" {
                let exists = Path::new(&source.locator).canonicalize().is_ok();
                let mut report = local(source.connection_id.clone(), false);
                if !exists {
                    report.configuration = SourceConfiguration::Missing;
                    report.reasons.push("local_source_path_unavailable".into());
                }
                reports.insert(source.connection_id.clone(), report);
            } else {
                reports.insert(
                    source.connection_id.clone(),
                    SourceSupport {
                        connection_id: source.connection_id.clone(),
                        provider: source.provider.clone(),
                        configuration: SourceConfiguration::Missing,
                        capabilities: None,
                        native_query: false,
                        evidence_read: false,
                        queue_observed: false,
                        claim_verification: false,
                        event_subscription: false,
                        reasons: vec!["connection_metadata_missing".into()],
                    },
                );
            }
        }
        for path in &checkout.stores {
            if Path::new(path).canonicalize().ok().as_deref() != Some(source_path) {
                use sha2::{Digest, Sha256};
                let id = format!("{:x}", Sha256::digest(path.as_bytes()));
                reports.entry(id.clone()).or_insert_with(|| {
                    let mut report = local(id, false);
                    if Path::new(path).canonicalize().is_err() {
                        report.configuration = SourceConfiguration::Missing;
                        report.reasons.push("local_source_path_unavailable".into());
                    }
                    report
                });
            }
        }
    }
    if let Some(selected) = selected {
        let mut report = local(selected.to_owned(), true);
        if reports.get(selected).is_some_and(|previous| {
            previous.provider != "git" || previous.configuration != SourceConfiguration::Ready
        }) {
            report.configuration = SourceConfiguration::Reconfigured;
            report
                .reasons
                .push("selected_source_metadata_conflict".into());
        }
        reports.insert(selected.to_owned(), report);
    }
    reports.into_values().collect()
}

fn provider_label(provider: &str) -> String {
    match provider {
        "github" => "GitHub".into(),
        "gitlab" => "GitLab".into(),
        "jira" => "Jira".into(),
        "git" => "Git".into(),
        other => other.to_owned(),
    }
}

fn last_component(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.to_owned(), str::to_owned)
}

/// One entry per uncovered source, in `uncovered` order (HS2-83KFB5). Names come from the
/// connection's own name, else its provider descriptor (`GitHub owner/repo`), else the
/// provider and locator; an unlinked local store is named by its directory. Metadata only.
pub(super) fn coverage_gaps(
    uncovered: &[String],
    reports: &[SourceSupport],
    connections: &[ProviderConnection],
    checkouts: &[Checkout],
    store_ids: &BTreeMap<String, String>,
) -> Vec<CoverageGap> {
    uncovered
        .iter()
        .map(|id| {
            if let Some(store) = store_ids.get(id) {
                return CoverageGap {
                    connection_id: id.clone(),
                    name: last_component(store),
                    provider: "git".into(),
                    reason: CoverageGapReason::Unobserved,
                    details: vec!["additional_local_store_not_observed".into()],
                };
            }
            let report = reports.iter().find(|report| &report.connection_id == id);
            let connection = connections.iter().find(|connection| &connection.id == id);
            let source = checkouts
                .iter()
                .flat_map(|checkout| &checkout.sources)
                .find(|source| &source.connection_id == id);
            let provider = report
                .map(|report| report.provider.clone())
                .or_else(|| connection.map(|c| c.provider.clone()))
                .or_else(|| source.map(|s| s.provider.clone()))
                .unwrap_or_else(|| "unknown".into());
            let name = connection
                .and_then(|c| c.name.clone().filter(|name| !name.trim().is_empty()))
                .or_else(|| {
                    connection
                        .and_then(|c| hotsheet_extsync::descriptor(c).ok())
                        .map(|descriptor| descriptor.display_name)
                })
                .or_else(|| {
                    let locator = connection
                        .map(|c| &c.locator)
                        .or(source.map(|s| &s.locator))?;
                    Some(if provider == "git" {
                        last_component(locator)
                    } else {
                        format!("{} {locator}", provider_label(&provider))
                    })
                })
                .unwrap_or_else(|| id.clone());
            let reason = match report {
                Some(report)
                    if matches!(
                        report.configuration,
                        SourceConfiguration::Missing
                            | SourceConfiguration::Invalid
                            | SourceConfiguration::Reconfigured
                    ) =>
                {
                    CoverageGapReason::FailedRead
                }
                Some(report) if report.provider == "git" => CoverageGapReason::Unobserved,
                Some(_) => CoverageGapReason::Unsupported,
                None => CoverageGapReason::Unobserved,
            };
            CoverageGap {
                connection_id: id.clone(),
                name,
                provider,
                reason,
                details: report.map_or_else(
                    || vec!["source_metadata_unknown".into()],
                    |report| report.reasons.clone(),
                ),
            }
        })
        .collect()
}
