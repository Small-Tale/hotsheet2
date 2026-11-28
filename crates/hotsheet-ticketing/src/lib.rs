//! Hot Sheet 2 ticketing engine — git store + index + watch + query + coordination.
//!
//! This is the only domain crate the CLI links (besides `hotsheet-model`); it must
//! **not** depend on the plugin crates or `hotsheet-terminals`
//! (`docs/12-code-organization-and-testing.md` §12.2.1). Its public API is
//! **synchronous** (docs/09 §9.12), and it reaches the outside world only through
//! the injected [`ports`].

use hotsheet_model::Ulid;

pub mod activity;
pub mod activity_distillation;
pub mod actor;
pub mod advisory_claim;
pub mod advisory_claim_ledger;
pub mod advisory_claim_v2;
pub mod analytics;
pub mod annotation_activity;
pub mod calibration;
pub mod checkout_order;
pub mod checkout_page;
pub mod claim_backends;
pub mod claim_codec;
pub mod claim_gate;
pub mod claim_host;
pub mod completion_remote;
pub mod corrupt_cache;
pub mod distclaim;
pub mod distwork;
pub mod features;
pub mod git;
mod git_discovery;
pub mod github_endpoint;
pub mod gitref_cas;
pub mod groups;
pub mod host_health;
pub mod host_health_disks;
pub mod identity;
pub mod image_crop;
pub mod line_endings;
pub mod live_history;
pub mod machine_home;
pub mod merge;
pub mod merge_driver;
pub mod metrics;
pub mod milestone_project;
pub mod milestone_rollup;
pub mod milestones;
pub mod ops;
pub mod orchestration;
pub mod overlay;
pub mod permission_automation;
pub mod phase_report;
pub mod phase_timing;
pub mod plugin_digest;
pub mod ports;
pub mod pricing;
pub mod project_gitignore;
pub mod project_time_zone;
pub mod provider;
pub mod provider_outbox;
pub mod provider_trash;
pub mod provisional;
pub mod registry;
pub mod remote_servers;
pub mod roster;
pub mod saved_views;
pub mod screensaver;
pub mod secrets;
pub mod settings;
pub mod slash_path;
pub mod standby;
pub mod store;
pub mod store_git;
pub mod store_transactions;
pub mod sync;
pub mod sync_ai;
pub mod sync_health;
pub mod sync_locks;
pub mod sync_replay;
pub mod sync_saved_writes;
pub mod sync_stale_locks;
pub mod terminal_limits;
pub mod wire;
pub mod worklist;
pub use activity::{
    ActivityEvent, ActivityKind, ActivityVolumeGuard, ActivityVolumePolicy, Importance,
    TimelineFilter,
};
pub use activity_distillation::{
    ActivityWindowProvenance, CandidateTrigger, DeterministicActivitySummarizer,
    DistillationPipeline, DistillationPolicy, DistillationRequest, DistilledActivityNote,
    LocalActivitySummarizer, SafeActivityEvent, write_distilled_note,
};
pub use auto_context::{AutoContextEntry, AutoContextSource, TicketAutoContext};
pub use corrupt_cache::CorruptTicketCache;
pub use distclaim::{ClaimMarker, ClaimResult, DistError};
pub use file_lock::FileLock;
pub use identity::{ME, current_user_email, current_user_name, resolve_me};
pub use merge::{BodyMerge, MergeOutcome, merge_tickets};
pub use metrics::{Rollup, UsageEvent};
pub use ops::{NewTicket, OpError, SortKey, TicketCollection, TicketPatch, TicketQuery};
pub use overlay::LocalOverlay;
pub use ports::{Clock, Rng};
pub use provider::{
    AiFeedbackRecord, GitProvider, MACHINE_STORE_CONFIG_DIR, MutationContext, NotWorkingReport,
    PROVIDER_CONFIG_FILE, PROVIDER_MIGRATION_FILE, ProjectTicketRef, ProviderCapabilities,
    ProviderConfigRegistry, ProviderConfigSeed, ProviderConnection, ProviderDescriptor,
    ProviderDraft, ProviderError, ProviderEvidence, ProviderKeysetItem, ProviderKeysetPage,
    ProviderMigrationEntry, ProviderMigrationLedger, ProviderMigrationReport,
    ProviderMigrationStore, ProviderMigrationWarning, ProviderMigrationWarningKind,
    ProviderMutationTiming, ProviderPatch, ProviderRegistry, ProviderTicketPage,
    ProviderTicketSummary, TicketProvider, TicketRef, TransferError, TransferOutcome,
    TransferProvenance, compare_provider_tickets, copy_between, filter_provider_ticket_page,
    generate_connection_id, git_connection_id, keyset_page_from_native_pages,
    keyset_page_from_rows, legacy_store_provider_config_path, machine_provider_config_path,
    migrate_machine_provider_config, move_between, provider_group_matches,
    provider_migration_ledger_path, provider_text_matches, store_config_key, unbounded_query,
};
pub use provider_trash::{
    ProviderPurgeOutcome, ProviderTrashPurge, RetainedTrashTicket, purge_provider_trash,
};
pub use registry::StoreRegistry;
pub use roster::{Person, Roster};
pub use secrets::{
    KeyMetadata, KeyMetadataCacheObservation, KeyRegistry, OsKeychain, ScopedOsKeychain,
    SecretError, SecretStore, key_metadata_cache_observation, resolve_setting_secret,
};
pub use settings::{
    AUTO_GROUP_NEW_TICKETS_SETTING, DEFAULT_AUTO_GROUP_NEW_TICKETS, DEFAULT_TRASH_CLEANUP_DAYS,
    Scope, Settings, SettingsError, TRASH_CLEANUP_DAYS_SETTING, parse_trash_cleanup_days,
};
pub use slash_path::slash_path;
pub use store::{
    AtomicAttachment, AttachmentNameRepairReport, AttachmentRename, CorruptTicket, FsStore,
    RefusedAttachmentTicket, RepairedAttachmentTicket, RepairedPayload, STORE_METADATA_FILE,
    STORE_SCHEMA_VERSION, StoreError, StoreListing, StoreMetadata, flush_background_commits,
    normalize_ticket_prefix, recover_ticket_identity, take_headless_published_stores,
};
pub use sync::{SyncReport, sync_once};
pub use sync_health::{SyncHealth, SyncState};
pub use wire::{ApiNote, ApiTicket, TicketRow};

/// Mint a new ticket ULID from an injected clock + rng.
///
/// The core never reads the wall clock or a global rng directly — both come through
/// ports so tests are deterministic (`docs/12` §12.1). IDs are ULIDs: no central
/// sequence, mintable offline, k-sortable (`docs/02` §2.4).
pub fn mint_ulid(clock: &dyn Clock, rng: &mut dyn Rng) -> Ulid {
    Ulid::from_parts(clock.now_ms(), rng.next_u128())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeClock(u64);
    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0
        }
    }

    struct FakeRng(u128);
    impl Rng for FakeRng {
        fn next_u128(&mut self) -> u128 {
            self.0
        }
    }

    #[test]
    fn mint_ulid_uses_injected_clock_and_rng() {
        let clock = FakeClock(1_469_922_850_259);
        let mut rng = FakeRng(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10);

        let a = mint_ulid(&clock, &mut rng);
        let b = mint_ulid(&clock, &mut rng);

        assert_eq!(
            a, b,
            "same clock+rng → same ULID under fakes (deterministic)"
        );
        assert_eq!(
            a.timestamp_ms(),
            1_469_922_850_259,
            "timestamp portion reflects the injected clock"
        );
    }
}
pub mod accounts;
pub mod auto_context;
pub mod checkout_removal;
pub mod checkouts;
pub mod command_cleanup;
pub mod command_schedules;
pub mod commands;
pub mod connection_removal;
mod file_lock;
pub mod github_origin;
pub mod repository_compaction;
pub mod repository_size;
pub mod repository_status;
mod settings_locks;
