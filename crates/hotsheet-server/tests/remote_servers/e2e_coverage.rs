//! HS2-30XYPY: two more scenarios on the HS2-2EY9DP fleet.
//!
//! - **Mixed ticket providers.** A third target on the peer whose ticket source is an
//!   external provider. A loopback provider stands in for the service: it answers as a
//!   `github` connection from a local store, so nothing leaves loopback. A
//!   dispatcher-mode run marks the ticket Up Next and a direct claim is refused as
//!   `target_operation_unsupported`.
//! - **A contended single-holder resource** (§26.20). A step on the peer requires a
//!   resource another holder claimed on the peer's machine. The loop defers it
//!   (`resource_held`) and places it once the claim is released.
use super::e2e::{ACTIONS, Fleet, Side, TARGET_A, TARGET_B, TOOL_A, TOOL_B};
use super::orchestration::shared_runtime;
use super::*;
use hotsheet_cli::orchestration::ExternalProviders;
use hotsheet_model::{CloseReason, NoteKind, ReviewRequest, Timestamp};
use hotsheet_ticketing::checkouts::{CheckoutRegistry, TicketSource};
use hotsheet_ticketing::{
    ApiTicket, GitProvider, MutationContext, ProviderConnection, ProviderDescriptor, ProviderDraft,
    ProviderError, ProviderPatch, StoreMetadata, TicketProvider, TicketQuery,
};
use std::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);
const TARGET_C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FK2";
const EXTERNAL: &str = "synthetic-issues";

/// A `github` connection answered from a local store: every call is the store's own,
/// only the descriptor names the external provider.
struct Loopback(GitProvider);

impl TicketProvider for Loopback {
    fn descriptor(&self) -> ProviderDescriptor {
        let mut descriptor = self.0.descriptor();
        descriptor.provider = "github".into();
        descriptor
    }
    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
        self.0.query(query)
    }
    fn find_transfer(&self, operation_id: &str) -> Result<Option<ApiTicket>, ProviderError> {
        self.0.find_transfer(operation_id)
    }
    fn get(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        self.0.get(native_id)
    }
    fn create(
        &self,
        ctx: MutationContext,
        draft: ProviderDraft,
    ) -> Result<ApiTicket, ProviderError> {
        self.0.create(ctx, draft)
    }
    fn update(
        &self,
        native_id: &str,
        now: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        self.0.update(native_id, now, patch)
    }
    fn add_note(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        self.0.add_note(native_id, ctx, kind, text)
    }
    fn close(
        &self,
        native_id: &str,
        now: Timestamp,
        reason: CloseReason,
        duplicate_of: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
        self.0.close(native_id, now, reason, duplicate_of)
    }
    fn assign(
        &self,
        native_id: &str,
        now: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError> {
        self.0.assign(native_id, now, assignees, reviews)
    }
    fn claim_next(
        &self,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
        label: Option<String>,
    ) -> Result<Option<ApiTicket>, ProviderError> {
        self.0.claim_next(now, lease_expires, worker, label)
    }
    fn release(
        &self,
        native_id: &str,
        now: Timestamp,
        worker: &str,
        force: bool,
    ) -> Result<ApiTicket, ProviderError> {
        self.0.release(native_id, now, worker, force)
    }
    fn renew(
        &self,
        native_id: &str,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
    ) -> Result<ApiTicket, ProviderError> {
        self.0.renew(native_id, now, lease_expires, worker)
    }
}

/// The peer's external providers: the loopback for its one external connection.
struct LoopbackProviders(std::path::PathBuf);

impl ExternalProviders for LoopbackProviders {
    fn open(
        &self,
        _: &hotsheet_ticketing::ProviderConfigRegistry,
        connection: &ProviderConnection,
    ) -> Result<Arc<dyn TicketProvider>, ProviderError> {
        Ok(Arc::new(Loopback(GitProvider::new(
            connection.id.clone(),
            FsStore::open(&self.0).map_err(|error| ProviderError::Authentication {
                connection_id: connection.id.clone(),
                message: error.to_string(),
            })?,
        ))))
    }
}

/// HS2-30XYPY: a target on the peer whose ticket source is an external provider. A
/// direct dispatch needs a claim the provider cannot take, so the peer refuses it as
/// unsupported and nothing is recorded; a dispatcher-mode run marks the ticket Up
/// Next through the provider, and cancelling the run withdraws the mark.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_30xypy_an_external_provider_target_is_queued_up_next_and_never_claimed() {
    let mut fleet = Fleet::new().await;
    // The peer's project C reads its tickets from an external connection; the peer's
    // orchestration runtime opens that connection through the loopback.
    let c_store = fleet.root.path().join("c-issues");
    FsStore::init_git(&c_store, &StoreMetadata::new("EXT")).unwrap();
    // HS2-1M8E9D: the peer machine's own copy, in the peer's home.
    let providers =
        hotsheet_ticketing::ProviderConfigRegistry::for_store_in(&fleet.peer_home, &fleet.b_store)
            .unwrap();
    providers
        .save(&[ProviderConnection {
            id: EXTERNAL.into(),
            provider: "github".into(),
            locator: "synthetic/issues".into(),
            name: None,
            default: false,
            settings: Value::Null,
            disabled: false,
        }])
        .unwrap();
    let c_project = fleet.root.path().join("c-project");
    super::sessions::git_project(&c_project);
    let registry = CheckoutRegistry::new(fleet.peer_home.join("checkouts.json"));
    let registered = registry
        .register(&c_project, None, None, Vec::new())
        .unwrap();
    let c_checkout = registry
        .add_registered_source(
            &providers,
            &registered.id,
            TicketSource {
                connection_id: EXTERNAL.into(),
                provider: "github".into(),
                locator: "synthetic/issues".into(),
            },
            false,
        )
        .unwrap();
    let peer_memory = Arc::new(Memory::default());
    let runtime = shared_runtime(&fleet.b_store, &fleet.peer_home, &peer_memory)
        .with_external_providers(Arc::new(LoopbackProviders(c_store.clone())));
    fleet.peer_offline().await;
    fleet.peer_state = fleet.peer_state.clone().with_orchestration_runtime(runtime);
    fleet.peer_api = app(fleet.peer_state.clone());
    fleet.peer_online().await;

    // The orchestration project declares and binds C on the peer.
    let manifest = fleet.ok("GET", "/targets", Value::Null).await;
    let added = fleet
        .ok(
            "POST",
            "/targets",
            json!({"target_id":TARGET_C,"label":"External target",
                "expected_revision":manifest["revision"]}),
        )
        .await;
    let candidate = fleet
        .ok(
            "POST",
            "/inspect",
            json!({"destination":{"kind":"remote","registration_id":ID,
                "checkout_id":c_checkout.id}}),
        )
        .await;
    let bound = fleet
        .ok(
            "PUT",
            &format!("/targets/{TARGET_C}/binding"),
            json!({"target":{"owner_checkout_id":fleet.owner.id,"target_id":TARGET_C},
                "expected_manifest_revision":added["revision"],"expected_binding_revision":0,
                "previous_destination":null,"destination":candidate["destination"]}),
        )
        .await;
    assert_eq!(bound["usable"], true, "{bound}");
    fleet.remember_tools(TARGET_C, &[TOOL_B]).await;
    fleet
        .policy(json!({"targets":[
            {"target_id":TARGET_A,"actions":ACTIONS,"tools":[{"tool":TOOL_A}],"max_sessions":1},
            {"target_id":TARGET_B,"actions":ACTIONS,"tools":[{"tool":TOOL_B}],"max_sessions":1},
            {"target_id":TARGET_C,"actions":ACTIONS,"tools":[{"tool":TOOL_B}],"max_sessions":1}]}))
        .await;

    let orchestration = fleet.orchestration_ticket("Coordinate an external tracker");
    let issue = hotsheet_ticketing::ops::create(
        &FsStore::open(&c_store).unwrap(),
        hotsheet_model::Ulid::new(),
        "EXT",
        "2026-10-15T00:00:00Z".into(),
        hotsheet_ticketing::NewTicket {
            title: "Synthetic external issue".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let dispatch = |operation: &str, mode: Option<&str>| {
        let mut body = json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
            "target_id":TARGET_C,"connection_id":EXTERNAL,"native_id":issue.slug,
            "operation_id":operation});
        // A direct run names its worker; the target's dispatcher chooses otherwise.
        match mode {
            Some(mode) => body["mode"] = json!(mode),
            None => body["worker_id"] = json!("worker-ext"),
        }
        body
    };
    // The external ticket reads through the gate like any other.
    let read = fleet
        .ok(
            "POST",
            "/tickets/read",
            json!({"actor":Fleet::ai(),"target_id":TARGET_C,"connection_id":EXTERNAL,
                "native_id":issue.slug}),
        )
        .await;
    assert_eq!(read["title"], "Synthetic external issue", "{read}");
    assert_eq!(read["connection_id"], EXTERNAL, "{read}");

    // A direct dispatch needs a claim the provider cannot take: refused, nothing kept.
    let (status, refused) = fleet
        .call("POST", "/runs", dispatch("op-ext-direct", None))
        .await;
    assert!(status.is_client_error(), "{status}: {refused}");
    assert_eq!(refused["code"], "target_operation_unsupported", "{refused}");
    let stored = FsStore::open(&c_store)
        .unwrap()
        .read_ticket(&issue.id)
        .unwrap();
    assert!(stored.claimed_by.is_none() && !stored.up_next);

    // Dispatcher mode: the run queues the ticket Up Next through the provider.
    let queued = fleet
        .ok(
            "POST",
            "/runs",
            dispatch("op-ext-queue", Some("dispatcher")),
        )
        .await;
    let run_id = queued["run_id"].as_str().unwrap().to_owned();
    assert_eq!(queued["mode"], "dispatcher", "{queued}");
    assert_eq!(queued["state"], "queued", "{queued}");
    let stored = FsStore::open(&c_store)
        .unwrap()
        .read_ticket(&issue.id)
        .unwrap();
    assert!(stored.up_next && stored.claimed_by.is_none());
    // The same request again is the same run.
    let again = fleet
        .ok(
            "POST",
            "/runs",
            dispatch("op-ext-queue", Some("dispatcher")),
        )
        .await;
    assert_eq!(again["run_id"], run_id.as_str());
    // Cancelling withdraws the Up Next mark.
    let cancelled = fleet
        .ok(
            "POST",
            &format!("/runs/{run_id}/cancel"),
            json!({"actor":Fleet::ai()}),
        )
        .await;
    assert_eq!(cancelled["state"], "cancelled", "{cancelled}");
    let stored = FsStore::open(&c_store)
        .unwrap()
        .read_ticket(&issue.id)
        .unwrap();
    assert!(!stored.up_next);
    fleet.shut_down().await;
}

/// HS2-30XYPY: a step on the peer needs a single-holder resource that another holder
/// claimed on the peer's machine. The loop defers it as `resource_held`, starting
/// nothing, and places it once the holder releases the claim.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn hs2_30xypy_a_held_resource_defers_the_step_until_its_release() {
    let mut fleet = Fleet::new().await;
    // Another holder takes the VM on the peer's machine, as a session would.
    let claims = hotsheet_resource_claims::ClaimService::new(&fleet.peer_home);
    let vm = hotsheet_resource_claims::ResourceRef::parse("vm/macos-vm").unwrap();
    let grant = claims
        .acquire(
            &vm,
            Duration::from_secs(1800),
            None,
            &std::collections::BTreeMap::new(),
        )
        .unwrap();
    let orchestration = fleet.orchestration_ticket("Coordinate work that needs the VM");
    let work = fleet.target_ticket(Side::Peer, "Synthetic change in the VM");
    let defined = fleet
        .ok(
            "POST",
            "/readiness/define",
            json!({"actor":Fleet::ai(),"ticket":{"native_id":orchestration.slug},
                "target_id":Fleet::target(Side::Peer),"connection_id":fleet.connection(Side::Peer),
                "native_id":work.slug,"prerequisites":[],
                "resources":[{"namespace":"vm","resource":"macos-vm"}]}),
        )
        .await;
    let step = defined["step"]["step_id"].as_str().unwrap().to_owned();
    fleet.automatic().await;
    let deferred = fleet
        .decided(
            "a deferral on the held VM",
            |entry| {
                entry["kind"] == "unplaced"
                    && entry["step_id"] == step.as_str()
                    && entry["reason"] == "resource_held"
            },
            MINUTE,
        )
        .await;
    assert_eq!(deferred["detail"], "vm/macos-vm", "{deferred}");
    assert!(fleet.drawer(Side::Peer).await.is_empty());
    assert!(fleet.sessions().await.is_empty());
    // The holder releases it: the loop is woken and places the step on the peer.
    claims.release(&grant.renewal_token, Some(&vm)).unwrap();
    let placed = super::e2e_loop::placed(&fleet, &step, Side::Peer).await;
    assert!(!placed.run_id.is_empty());
    fleet.shut_down().await;
}
