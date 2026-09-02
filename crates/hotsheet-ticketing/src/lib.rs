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
pub mod analytics;
pub mod distclaim;
pub mod distwork;
pub mod identity;
pub mod merge;
pub mod metrics;
pub mod ops;
pub mod overlay;
pub mod ports;
pub mod pricing;
pub mod provider;
pub mod registry;
pub mod roster;
pub mod secrets;
pub mod settings;
pub mod store;
pub mod sync;
pub mod wire;
pub mod worklist;
pub use activity::{ActivityEvent, ActivityKind, Importance, TimelineFilter};
pub use activity_distillation::{
    ActivityWindowProvenance, CandidateTrigger, DeterministicActivitySummarizer,
    DistillationPipeline, DistillationPolicy, DistillationRequest, DistilledActivityNote,
    LocalActivitySummarizer, SafeActivityEvent, write_distilled_note,
};
pub use auto_context::{AutoContextEntry, AutoContextSource, TicketAutoContext};
pub use distclaim::{ClaimMarker, ClaimResult, DistError};
pub use identity::{ME, current_user_email, current_user_name, resolve_me};
pub use merge::{BodyMerge, MergeOutcome, merge_tickets};
pub use metrics::{Rollup, UsageEvent};
pub use ops::{NewTicket, OpError, SortKey, TicketPatch, TicketQuery};
pub use overlay::LocalOverlay;
pub use ports::{Clock, Rng};
pub use provider::{
    GitProvider, MutationContext, NotWorkingReport, ProviderCapabilities, ProviderConfigRegistry,
    ProviderConnection, ProviderDescriptor, ProviderDraft, ProviderError, ProviderEvidence,
    ProviderPatch, ProviderRegistry, TicketProvider, TicketRef, TransferError, TransferOutcome,
    TransferProvenance, copy_between, git_connection_id, move_between,
};
pub use registry::StoreRegistry;
pub use roster::{Person, Roster};
pub use secrets::{
    KeyMetadata, KeyRegistry, OsKeychain, SecretError, SecretStore, resolve_setting_secret,
};
pub use settings::{Scope, Settings, SettingsError};
pub use store::{
    AtomicAttachment, CorruptTicket, FsStore, STORE_SCHEMA_VERSION, StoreError, StoreListing,
    StoreMetadata,
};
pub use sync::{SyncReport, sync_once};
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
pub mod auto_context;
pub mod checkouts;
pub mod commands;
pub mod repository_status;
