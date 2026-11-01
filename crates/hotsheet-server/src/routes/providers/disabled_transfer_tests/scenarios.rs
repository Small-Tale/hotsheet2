use super::*;
use hotsheet_model::{CloseReason, NoteKind, ReviewRequest, Timestamp};
use hotsheet_ticketing::{
    MutationContext, ProviderDescriptor, ProviderDraft, ProviderError, ProviderPatch,
    TicketProvider,
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

struct Rendezvous {
    ready: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

struct OwnedWriter(Option<std::thread::JoinHandle<()>>);
impl Drop for OwnedWriter {
    fn drop(&mut self) {
        // Both channel waits are bounded, including early handler refusal/panic.
        if let Some(writer) = self.0.take() {
            let _ = writer.join();
        }
    }
}

struct Counted {
    inner: GitProvider,
    kind: &'static str,
    calls: [AtomicUsize; 3],
    descriptors: AtomicUsize,
    rendezvous: Mutex<Option<Rendezvous>>,
}

impl Counted {
    fn new(id: &str, store: FsStore, kind: &'static str) -> Arc<Self> {
        Arc::new(Self {
            inner: GitProvider::new(id, store).with_default(true),
            kind,
            calls: std::array::from_fn(|_| AtomicUsize::new(0)),
            descriptors: AtomicUsize::new(0),
            rendezvous: Mutex::new(None),
        })
    }
    fn counts(&self) -> [usize; 3] {
        std::array::from_fn(|index| self.calls[index].load(Ordering::SeqCst))
    }
    fn arm(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (ready, observed) = mpsc::channel();
        let (release, released) = mpsc::channel();
        *self.rendezvous.lock().unwrap() = Some(Rendezvous {
            ready,
            release: released,
        });
        self.descriptors.store(0, Ordering::SeqCst);
        (observed, release)
    }
}

impl TicketProvider for Counted {
    fn descriptor(&self) -> ProviderDescriptor {
        let visit = self.descriptors.fetch_add(1, Ordering::SeqCst) + 1;
        if visit == 2 {
            let rendezvous = self.rendezvous.lock().unwrap().take();
            if let Some(rendezvous) = rendezvous {
                rendezvous.ready.send(()).unwrap();
                rendezvous
                    .release
                    .recv_timeout(Duration::from_secs(5))
                    .expect("owned policy writer did not acknowledge");
            }
        }
        let mut descriptor = self.inner.descriptor();
        descriptor.provider = self.kind.into();
        descriptor
    }
    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
        self.calls[0].fetch_add(1, Ordering::SeqCst);
        self.inner.query(query)
    }
    fn find_transfer(&self, operation: &str) -> Result<Option<ApiTicket>, ProviderError> {
        self.calls[0].fetch_add(1, Ordering::SeqCst);
        self.inner.find_transfer(operation)
    }
    fn get(&self, id: &str) -> Result<ApiTicket, ProviderError> {
        self.calls[0].fetch_add(1, Ordering::SeqCst);
        self.inner.get(id)
    }
    fn create(
        &self,
        context: MutationContext,
        draft: ProviderDraft,
    ) -> Result<ApiTicket, ProviderError> {
        self.calls[1].fetch_add(1, Ordering::SeqCst);
        self.inner.create(context, draft)
    }
    fn close(
        &self,
        id: &str,
        at: Timestamp,
        reason: CloseReason,
        duplicate: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
        self.calls[2].fetch_add(1, Ordering::SeqCst);
        self.inner.close(id, at, reason, duplicate)
    }
    fn update(
        &self,
        id: &str,
        at: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        self.calls[1].fetch_add(1, Ordering::SeqCst);
        self.inner.update(id, at, patch)
    }
    fn add_note(
        &self,
        id: &str,
        context: MutationContext,
        kind: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        self.calls[1].fetch_add(1, Ordering::SeqCst);
        self.inner.add_note(id, context, kind, text)
    }
    fn assign(
        &self,
        id: &str,
        at: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError> {
        self.calls[1].fetch_add(1, Ordering::SeqCst);
        self.inner.assign(id, at, assignees, reviews)
    }
    fn claim_next(
        &self,
        at: Timestamp,
        expiry: Timestamp,
        worker: &str,
        label: Option<String>,
    ) -> Result<Option<ApiTicket>, ProviderError> {
        self.calls[1].fetch_add(1, Ordering::SeqCst);
        self.inner.claim_next(at, expiry, worker, label)
    }
    fn release(
        &self,
        id: &str,
        at: Timestamp,
        worker: &str,
        force: bool,
    ) -> Result<ApiTicket, ProviderError> {
        self.calls[1].fetch_add(1, Ordering::SeqCst);
        self.inner.release(id, at, worker, force)
    }
    fn renew(
        &self,
        id: &str,
        at: Timestamp,
        expiry: Timestamp,
        worker: &str,
    ) -> Result<ApiTicket, ProviderError> {
        self.calls[1].fetch_add(1, Ordering::SeqCst);
        self.inner.renew(id, at, expiry, worker)
    }
}

fn connection(id: &str, kind: &str, disabled: bool, account: &str) -> ProviderConnection {
    ProviderConnection {
        id: id.into(),
        provider: kind.into(),
        locator: if kind == "jira" {
            "SYN"
        } else {
            "synthetic/project"
        }
        .into(),
        name: None,
        default: false,
        disabled,
        settings: serde_json::json!({"credential":{"secret":account},"base_url":"https://synthetic.test","email":"fixture@example.test"}),
    }
}

fn records(config: &ProviderConfigRegistry, values: Vec<ProviderConnection>) {
    config
        .update(|current| {
            *current = values;
            Ok(())
        })
        .unwrap();
}

fn body(source: &str, native: &str, destination: &str, operation: &str) -> serde_json::Value {
    serde_json::json!({"source":{"connection_id":source,"native_id":native},"destination_connection":destination,"operation_id":operation,"confirm":true})
}

fn disabled(status: StatusCode, value: &serde_json::Value, id: &str) {
    assert_eq!(status, StatusCode::CONFLICT, "{value}");
    let expected = ProviderError::Disabled {
        connection_id: id.into(),
    }
    .to_string();
    assert_eq!(value["error"], expected);
}

async fn seed(state: &AppState, title: &str) -> String {
    let (status, value) = request(state, "/tickets", serde_json::json!({"title":title})).await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    value["id"].as_str().unwrap().into()
}

pub(super) async fn run(state: &AppState, config: &ProviderConfigRegistry, home: &Path) {
    let destination = init_store(&home.join("destination"), "DS");
    let (status, registered) = request(
        state,
        "/stores",
        serde_json::json!({"path":destination.root()}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{registered}");
    let destination_id = registered["id"].as_str().unwrap();
    let local = multistore::store_url_id(&state.store);
    let source = Counted::new("external-source", state.store.clone(), "gitlab");
    let target = Counted::new("external-destination", destination.clone(), "jira");
    let injected = state
        .clone()
        .with_ticket_provider(source.clone())
        .with_ticket_provider(target.clone());
    let native = seed(state, "Synthetic transfer admission").await;
    hs2_djwtb0_initial_selected_policy(&injected, config, &source, &target, &native, &destination)
        .await;
    hs2_djwtb0_replay_reenable(&injected, config, &source, &target, &native, &destination).await;
    hs2_djwtb0_routing_and_missing(
        state,
        config,
        &source,
        &target,
        &local,
        destination_id,
        &native,
    )
    .await;
    for change_kind in [false, true] {
        let fresh = seed(state, "Fresh final admission source").await;
        hs2_djwtb0_final_policy_observation(
            &injected,
            config,
            &source,
            &target,
            &fresh,
            change_kind,
        )
        .await;
    }
}

async fn hs2_djwtb0_initial_selected_policy(
    state: &AppState,
    config: &ProviderConfigRegistry,
    source: &Counted,
    target: &Counted,
    native: &str,
    destination: &FsStore,
) {
    // Both positions must be admitted before resolving even the enabled peer's missing credential.
    for kind in ["github", "jira", "gitlab", "git"] {
        for deny_source in [true, false] {
            let denied_id = if deny_source {
                "external-source"
            } else {
                "external-destination"
            };
            records(
                config,
                vec![
                    connection(
                        "external-source",
                        if deny_source { kind } else { "jira" },
                        deny_source,
                        MISSING_ACCOUNT,
                    ),
                    connection(
                        "external-destination",
                        if deny_source { "jira" } else { kind },
                        !deny_source,
                        MISSING_ACCOUNT,
                    ),
                ],
            );
            let before_calls = (source.counts(), target.counts());
            let before_config = std::fs::read(config.path()).unwrap();
            let before_source =
                std::fs::read(state.store.ticket_path(&native.parse().unwrap())).unwrap();
            let before_count = destination.list_tickets().unwrap().len();
            for route in ["/provider-transfers/copy", "/provider-transfers/move"] {
                let (status, value) = request(
                    state,
                    route,
                    body(
                        "external-source",
                        native,
                        "external-destination",
                        "initial-denied",
                    ),
                )
                .await;
                disabled(status, &value, denied_id);
            }
            assert_eq!((source.counts(), target.counts()), before_calls);
            assert_eq!(std::fs::read(config.path()).unwrap(), before_config);
            assert_eq!(
                std::fs::read(state.store.ticket_path(&native.parse().unwrap())).unwrap(),
                before_source
            );
            assert_eq!(destination.list_tickets().unwrap().len(), before_count);
        }
    }
    let mut unconfirmed = body(
        "external-source",
        native,
        "external-destination",
        "unconfirmed",
    );
    unconfirmed["confirm"] = false.into();
    let before = (source.counts(), target.counts());
    let (status, value) = request(state, "/provider-transfers/move", unconfirmed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{value}");
    assert_eq!((source.counts(), target.counts()), before);
}

async fn hs2_djwtb0_replay_reenable(
    state: &AppState,
    config: &ProviderConfigRegistry,
    source: &Counted,
    target: &Counted,
    native: &str,
    destination: &FsStore,
) {
    let mut unrelated = connection("unrelated-disabled", "jira", true, MISSING_ACCOUNT);
    unrelated.default = true;
    records(
        config,
        vec![
            connection("external-source", "github", false, ACCOUNT),
            connection("external-destination", "jira", false, ACCOUNT),
            unrelated,
        ],
    );
    let transfer = body(
        "external-source",
        native,
        "external-destination",
        "replay-reenable",
    );
    let before = destination.list_tickets().unwrap().len();
    let (status, first) = request(state, "/provider-transfers/copy", transfer.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    for id in ["external-source", "external-destination"] {
        config.set_disabled(id, true).unwrap();
        let calls = (source.counts(), target.counts());
        for route in ["/provider-transfers/copy", "/provider-transfers/move"] {
            let (status, value) = request(state, route, transfer.clone()).await;
            disabled(status, &value, id);
        }
        assert_eq!((source.counts(), target.counts()), calls);
        config.set_disabled(id, false).unwrap();
        let (status, repeated) = request(state, "/provider-transfers/copy", transfer.clone()).await;
        assert_eq!(status, StatusCode::CREATED, "{repeated}");
        assert_eq!(repeated["destination"], first["destination"]);
        assert_eq!(destination.list_tickets().unwrap().len(), before + 1);
    }
    let (status, moved) = request(state, "/provider-transfers/move", transfer).await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["destination"], first["destination"]);
    assert_eq!(moved["moved"], true);
    assert_eq!(source.counts()[2], 1);
    assert_eq!(
        state
            .store
            .read_ticket(&native.parse().unwrap())
            .unwrap()
            .close_reason,
        Some(CloseReason::Obsolete)
    );
}

async fn hs2_djwtb0_routing_and_missing(
    state: &AppState,
    config: &ProviderConfigRegistry,
    source: &Arc<Counted>,
    target: &Arc<Counted>,
    local: &str,
    destination: &str,
    native: &str,
) {
    // No persisted row: injection remains legitimate, even with an unrelated disabled default.
    records(
        config,
        vec![connection(
            "unrelated-disabled",
            "gitlab",
            true,
            MISSING_ACCOUNT,
        )],
    );
    let injected = state
        .clone()
        .with_ticket_provider(source.clone())
        .with_ticket_provider(target.clone());
    let registry =
        hosted_provider_registry(&injected, "external-source", "external-source").unwrap();
    let descriptor = registry.get("external-source").unwrap().descriptor();
    assert_eq!(descriptor.provider, "gitlab");
    assert!(descriptor.default);
    assert_eq!(registry.descriptors().len(), 1);
    let same_native = seed(state, "Same selected source").await;
    let (status, value) = request(
        &injected,
        "/provider-transfers/copy",
        body(
            "external-source",
            &same_native,
            "external-source",
            "same-selected",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{value}");

    records(
        config,
        vec![connection(local, "git", true, MISSING_ACCOUNT)],
    );
    assert_eq!(
        hosted_provider_registry(state, local, destination)
            .unwrap()
            .get(local)
            .unwrap()
            .descriptor()
            .provider,
        "git"
    );
    let collision = Counted::new(local, state.store.clone(), "jira");
    let overridden = state.clone().with_ticket_provider(collision.clone());
    let (status, value) = request(
        &overridden,
        "/provider-transfers/copy",
        body(local, native, destination, "local-external-collision"),
    )
    .await;
    disabled(status, &value, local);
    assert_eq!(collision.counts(), [0; 3]);
    config.set_disabled(local, false).unwrap();
    assert_eq!(
        hosted_provider_registry(&overridden, local, destination)
            .unwrap()
            .get(local)
            .unwrap()
            .descriptor()
            .provider,
        "jira"
    );

    // Persisted external policy still applies to an injected Git winner.
    let git_override = Counted::new(local, state.store.clone(), "git");
    let git_state = state.clone().with_ticket_provider(git_override.clone());
    records(
        config,
        vec![connection(local, "jira", true, MISSING_ACCOUNT)],
    );
    let (status, value) = request(
        &git_state,
        "/provider-transfers/copy",
        body(local, native, destination, "external-git-collision"),
    )
    .await;
    disabled(status, &value, local);
    assert_eq!(git_override.counts(), [0; 3]);

    // A disabled Git row does not disable the actual injected Git winner.
    records(
        config,
        vec![connection(local, "git", true, MISSING_ACCOUNT)],
    );
    let winner = hosted_provider_registry(&git_state, local, destination)
        .unwrap()
        .get(local)
        .unwrap();
    assert_eq!(winner.descriptor().provider, "git");
    assert!(
        winner.descriptor().default,
        "the injected Git object, not the hosted default, wins"
    );
    let git_native = seed(state, "Disabled Git metadata with a local Git winner").await;
    let source_bytes =
        std::fs::read(state.store.ticket_path(&git_native.parse().unwrap())).unwrap();
    let config_bytes = std::fs::read(config.path()).unwrap();
    let destination_store = state.host.get(destination).unwrap().store.clone();
    let destination_count = destination_store.list_tickets().unwrap().len();
    let calls = git_override.counts();
    let transfer = body(local, &git_native, destination, "disabled-git-local-copy");
    let (status, first) = request(&git_state, "/provider-transfers/copy", transfer.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["destination"]["connection_id"], destination);
    let (status, repeated) = request(&git_state, "/provider-transfers/copy", transfer).await;
    assert_eq!(status, StatusCode::CREATED, "{repeated}");
    assert_eq!(repeated["destination"], first["destination"]);
    assert_eq!(
        destination_store.list_tickets().unwrap().len(),
        destination_count + 1
    );
    assert_eq!(
        git_override.counts(),
        [calls[0] + 2, calls[1], calls[2]],
        "both real copies read the injected Git winner without writing or closing it"
    );
    assert_eq!(
        std::fs::read(state.store.ticket_path(&git_native.parse().unwrap())).unwrap(),
        source_bytes
    );
    assert_eq!(std::fs::read(config.path()).unwrap(), config_bytes);

    // A persisted Git row without any hosted/injected object remains genuinely unknown.
    const SKIPPED: &str = "persisted-git-without-object";
    for flag in [false, true] {
        records(
            config,
            vec![connection(SKIPPED, "git", flag, MISSING_ACCOUNT)],
        );
        let registry = hosted_provider_registry(&git_state, SKIPPED, destination).unwrap();
        assert!(
            matches!(registry.get(SKIPPED), Err(ProviderError::UnknownConnection(id)) if id == SKIPPED)
        );
        let calls = (source.counts(), target.counts(), git_override.counts());
        let config_bytes = std::fs::read(config.path()).unwrap();
        let source_bytes =
            std::fs::read(state.store.ticket_path(&git_native.parse().unwrap())).unwrap();
        let destination_count = destination_store.list_tickets().unwrap().len();
        for (from, to) in [(SKIPPED, destination), (local, SKIPPED)] {
            let (status, value) = request(
                &git_state,
                "/provider-transfers/copy",
                body(from, &git_native, to, "skipped-git-missing"),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT, "{value}");
            assert_eq!(
                value["error"],
                ProviderError::UnknownConnection(SKIPPED.into()).to_string()
            );
        }
        assert_eq!(
            (source.counts(), target.counts(), git_override.counts()),
            calls
        );
        assert_eq!(
            destination_store.list_tickets().unwrap().len(),
            destination_count
        );
        assert_eq!(
            std::fs::read(state.store.ticket_path(&git_native.parse().unwrap())).unwrap(),
            source_bytes
        );
        assert_eq!(std::fs::read(config.path()).unwrap(), config_bytes);
    }

    records(config, vec![]);
    for (from, to) in [
        ("missing-selected", destination),
        (local, "missing-selected"),
    ] {
        let before = state.store.read_ticket(&native.parse().unwrap()).unwrap();
        let (status, value) = request(
            state,
            "/provider-transfers/copy",
            body(from, native, to, "missing-selected"),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert!(
            value["error"]
                .as_str()
                .unwrap()
                .contains("missing-selected")
        );
        assert_eq!(
            state
                .store
                .read_ticket(&native.parse().unwrap())
                .unwrap()
                .close_reason,
            before.close_reason
        );
    }
}

async fn hs2_djwtb0_final_policy_observation(
    state: &AppState,
    config: &ProviderConfigRegistry,
    source: &Arc<Counted>,
    target: &Counted,
    native: &str,
    change_kind: bool,
) {
    records(
        config,
        vec![connection(
            "external-source",
            if change_kind { "jira" } else { "git" },
            false,
            ACCOUNT,
        )],
    );
    let (observed, release) = source.arm();
    let path = config.path().to_path_buf();
    let mut writer = OwnedWriter(Some(std::thread::spawn(move || {
        observed
            .recv_timeout(Duration::from_secs(5))
            .expect("registration descriptor was not reached after initial admission");
        let registry = ProviderConfigRegistry::new(path);
        registry
            .update(|records| {
                let record = records
                    .iter_mut()
                    .find(|record| record.id == "external-source")
                    .unwrap();
                record.disabled = true;
                if change_kind {
                    record.provider = "git".into();
                }
                Ok(())
            })
            .unwrap();
        release.send(()).unwrap();
    })));
    let calls = (source.counts(), target.counts());
    let (status, value) = request(
        state,
        "/provider-transfers/copy",
        body(
            "external-source",
            native,
            "external-destination",
            if change_kind {
                "final-reclassified"
            } else {
                "final-toggle"
            },
        ),
    )
    .await;
    writer.0.take().unwrap().join().unwrap();
    disabled(status, &value, "external-source");
    assert!(
        source.descriptors.load(Ordering::SeqCst) >= 3,
        "snapshot, registration, and constructed winner were observed"
    );
    assert_eq!((source.counts(), target.counts()), calls);
    assert!(config.disabled("external-source").unwrap());
    config.set_disabled("external-source", false).unwrap();
    let (status, value) = request(
        state,
        "/provider-transfers/copy",
        body(
            "external-source",
            native,
            "external-destination",
            if change_kind {
                "final-reclassified"
            } else {
                "final-toggle"
            },
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "re-enable after final refusal: {value}"
    );
}
