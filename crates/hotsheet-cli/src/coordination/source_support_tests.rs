use super::contract::CoverageGapReason;
use super::contract::SourceConfiguration;
use super::source_support::{coverage_gaps, reports};
use super::*;
use hotsheet_ticketing::{ProviderConfigRegistry, ProviderConnection};
use serde_json::json;

fn connection(id: &str, provider: &str) -> ProviderConnection {
    ProviderConnection {
        id: id.into(),
        provider: provider.into(),
        locator: if provider == "jira" {
            "APP"
        } else {
            "synthetic/app"
        }
        .into(),
        name: None,
        default: false,
        disabled: false,
        settings: json!({"base_url":"http://127.0.0.1:9", "api_base":"http://127.0.0.1:9", "email":"fixture@example.test", "credential":{"secret":"must-not-resolve"}}),
    }
}

fn checkout(sources: Vec<checkouts::TicketSource>) -> checkouts::Checkout {
    serde_json::from_value(
        json!({"id":"synthetic", "root":"unused", "alias":"synthetic", "sources":sources}),
    )
    .unwrap()
}

#[test]
fn hs2_fg7zmf_metadata_support_uses_canonical_capabilities_without_claim_or_event_authority() {
    let path = tempfile::tempdir().unwrap();
    let connections: Vec<_> = ["github", "jira", "gitlab"]
        .into_iter()
        .map(|p| connection(p, p))
        .collect();
    let report = reports(Some("selected"), path.path(), &connections, &[]);
    assert_eq!(report.len(), 4);
    for connection in &connections {
        let entry = report
            .iter()
            .find(|r| r.connection_id == connection.id)
            .unwrap();
        let canonical = hotsheet_extsync::descriptor(connection)
            .unwrap()
            .capabilities;
        assert_eq!(entry.capabilities.as_ref(), Some(&canonical));
        assert!(entry.native_query && entry.evidence_read);
        assert!(!canonical.claims && !canonical.watch);
        assert!(!entry.queue_observed && !entry.claim_verification && !entry.event_subscription);
        assert!(
            entry
                .reasons
                .iter()
                .any(|r| r == "native_claims_unsupported")
        );
        assert!(
            entry
                .reasons
                .iter()
                .any(|r| r == "coordination_events_unavailable")
        );
    }
    let selected = report
        .iter()
        .find(|r| r.connection_id == "selected")
        .unwrap();
    assert!(selected.capabilities.as_ref().unwrap().watch);
    assert!(selected.queue_observed && selected.claim_verification && selected.event_subscription);
    assert!(selected.reasons.is_empty());
}

#[test]
fn hs2_fg7zmf_disabled_invalid_missing_reconfigured_and_reset_metadata_stay_distinct() {
    let path = tempfile::tempdir().unwrap();
    let mut c = connection("source", "github");
    let linked = checkout(vec![checkouts::TicketSource {
        connection_id: c.id.clone(),
        provider: c.provider.clone(),
        locator: c.locator.clone(),
    }]);
    let read = |connections: &[ProviderConnection], linked: &checkouts::Checkout| {
        reports(None, path.path(), connections, std::slice::from_ref(linked))
    };
    assert_eq!(
        read(&[c.clone()], &linked)[0].configuration,
        SourceConfiguration::Ready
    );
    c.disabled = true;
    assert_eq!(
        read(&[c.clone()], &linked)[0].configuration,
        SourceConfiguration::Disabled
    );
    c.disabled = false;
    c.locator = "synthetic/other".into();
    assert_eq!(
        read(&[c.clone()], &linked)[0].configuration,
        SourceConfiguration::Reconfigured
    );
    let absent = read(&[], &linked);
    assert_eq!(absent[0].configuration, SourceConfiguration::Missing);
    assert!(!absent[0].native_query && absent[0].capabilities.is_none());
    c.locator = "invalid".into();
    let invalid = read(&[c.clone()], &checkout(vec![]));
    assert_eq!(invalid[0].configuration, SourceConfiguration::Invalid);
    assert!(invalid[0].capabilities.is_none());
    c.provider = "unknown-provider".into();
    assert_eq!(
        read(&[c], &checkout(vec![]))[0].configuration,
        SourceConfiguration::Invalid
    );
    assert!(reports(None, path.path(), &[], &[]).is_empty());
    let again = connection("source", "github");
    assert_eq!(
        read(&[again], &linked)[0].configuration,
        SourceConfiguration::Ready
    );
}

#[test]
fn hs2_fg7zmf_additional_git_paths_are_described_without_reading_their_tickets() {
    let path = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    // Deliberately not a store: path metadata must not open a ticket provider.
    fs::write(
        other.path().join("tickets"),
        "not a readable ticket directory",
    )
    .unwrap();
    let linked = checkout(vec![checkouts::TicketSource {
        connection_id: "other".into(),
        provider: "git".into(),
        locator: other.path().display().to_string(),
    }]);
    let result = reports(Some("selected"), path.path(), &[], &[linked]);
    let other = result.iter().find(|r| r.connection_id == "other").unwrap();
    assert_eq!(other.configuration, SourceConfiguration::Ready);
    assert!(other.capabilities.as_ref().unwrap().claims);
    assert!(!other.queue_observed && !other.claim_verification && !other.event_subscription);
}

#[test]
fn hs2_fg7zmf_snapshot_metadata_is_additive_and_never_changes_state_or_covers_sources() {
    let (_root, c) = super::tests::fixture();
    let connection = connection("source", "github");
    let config = ProviderConfigRegistry::new(c.source.join("providers.json"));
    config.save(std::slice::from_ref(&connection)).unwrap();
    let bytes = fs::read(c.directory.join("state.json")).unwrap();
    let first = c.snapshot(1_800_000_000).unwrap();
    assert_eq!(first.uncovered_sources, ["source"]);
    assert_eq!(bytes, fs::read(c.directory.join("state.json")).unwrap());
    let mut old = serde_json::to_value(&first).unwrap();
    old.as_object_mut().unwrap().remove("source_support");
    assert!(
        serde_json::from_value::<Snapshot>(old)
            .unwrap()
            .source_support
            .is_empty()
    );
    config.set_disabled("source", true).unwrap();
    let disabled = c.snapshot(1_800_000_000).unwrap();
    assert!(disabled.uncovered_sources.is_empty());
    assert_ne!(disabled.cursor, first.cursor);
    config.set_disabled("source", false).unwrap();
    assert_eq!(first.cursor, c.snapshot(1_800_000_000).unwrap().cursor);
    assert!(
        c.unsupported_source("unknown")
            .unwrap()
            .contains("unknown to the configured source metadata")
    );
    assert_eq!(bytes, fs::read(c.directory.join("state.json")).unwrap());
    assert_eq!(c.state().unwrap().version, 3);
}

/// HS2-83KFB5: each uncovered source appears once, by its human-readable name, with a reason
/// that separates unsupported-by-design, failed reads and unobserved sources.
#[test]
fn hs2_83kfb5_coverage_gaps_name_each_source_once_with_a_reason() {
    let path = tempfile::tempdir().unwrap();
    let github = connection("github-synthetic", "github");
    let mut named = connection("jira-named", "jira");
    named.name = Some("Synthetic Jira".into());
    let mut broken = connection("github-broken", "github");
    broken.settings = json!({});
    broken.locator = String::new();
    let other_git = tempfile::tempdir().unwrap();
    let linked = checkout(vec![
        // The same GitHub source linked by the checkout and configured in providers.json.
        checkouts::TicketSource {
            connection_id: github.id.clone(),
            provider: github.provider.clone(),
            locator: github.locator.clone(),
        },
        checkouts::TicketSource {
            connection_id: "git-other".into(),
            provider: "git".into(),
            locator: other_git
                .path()
                .join("other.hs2")
                .to_string_lossy()
                .into_owned(),
        },
        checkouts::TicketSource {
            connection_id: "gitlab-unconfigured".into(),
            provider: "gitlab".into(),
            locator: "synthetic/unconfigured".into(),
        },
    ]);
    let connections = vec![github.clone(), named, broken];
    let checkouts = vec![linked];
    let support = reports(Some("selected"), path.path(), &connections, &checkouts);
    let mut store_ids = std::collections::BTreeMap::new();
    store_ids.insert(
        "store-hash".to_owned(),
        "/synthetic/stores/extra.hs2".to_owned(),
    );
    let uncovered: Vec<String> = [
        "git-other",
        "github-broken",
        "github-synthetic",
        "gitlab-unconfigured",
        "jira-named",
        "store-hash",
    ]
    .map(str::to_owned)
    .to_vec();
    let gaps = coverage_gaps(&uncovered, &support, &connections, &checkouts, &store_ids);
    let by_id = |id: &str| gaps.iter().find(|gap| gap.connection_id == id).unwrap();
    assert_eq!(
        gaps.len(),
        uncovered.len(),
        "one entry per uncovered source: {gaps:#?}"
    );
    assert_eq!(
        gaps.iter()
            .filter(|gap| gap.connection_id == "github-synthetic")
            .count(),
        1
    );
    let github_gap = by_id("github-synthetic");
    assert_eq!(github_gap.name, "GitHub synthetic/app");
    assert_eq!(github_gap.reason, CoverageGapReason::Unsupported);
    assert!(
        github_gap
            .details
            .iter()
            .any(|d| d == "native_claims_unsupported")
    );
    assert_eq!(by_id("jira-named").name, "Synthetic Jira");
    assert_eq!(by_id("jira-named").reason, CoverageGapReason::Unsupported);
    let broken_gap = by_id("github-broken");
    assert_eq!(broken_gap.reason, CoverageGapReason::FailedRead);
    assert!(
        broken_gap
            .details
            .iter()
            .any(|d| d == "provider_descriptor_unavailable")
    );
    // A linked provider with no configuration cannot be read; it is not "unsupported".
    let unconfigured = by_id("gitlab-unconfigured");
    assert_eq!(unconfigured.reason, CoverageGapReason::FailedRead);
    assert_eq!(unconfigured.name, "GitLab synthetic/unconfigured");
    let git_gap = by_id("git-other");
    assert_eq!(
        git_gap.reason,
        CoverageGapReason::FailedRead,
        "its path does not exist yet"
    );
    assert_eq!(git_gap.name, "other.hs2");
    let store_gap = by_id("store-hash");
    assert_eq!(store_gap.reason, CoverageGapReason::Unobserved);
    assert_eq!(store_gap.name, "extra.hs2");
    // Once the other Git source exists, it is a supported source this pool does not observe.
    std::fs::create_dir_all(other_git.path().join("other.hs2")).unwrap();
    let support = reports(Some("selected"), path.path(), &connections, &checkouts);
    let gaps = coverage_gaps(&uncovered, &support, &connections, &checkouts, &store_ids);
    let git_gap = gaps
        .iter()
        .find(|gap| gap.connection_id == "git-other")
        .unwrap();
    assert_eq!(git_gap.reason, CoverageGapReason::Unobserved);
    assert!(
        git_gap
            .details
            .iter()
            .any(|d| d == "additional_local_source_not_observed")
    );
    // Nothing uncovered, nothing reported.
    assert!(coverage_gaps(&[], &support, &connections, &checkouts, &store_ids).is_empty());
}
