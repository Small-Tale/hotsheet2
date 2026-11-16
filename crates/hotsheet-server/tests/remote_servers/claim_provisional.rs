//! HS2-XYF4YT: an uncoordinated claim of a dependent whose prerequisite awaits CI, over a
//! real server socket (default-store and checkout routes), through the MCP shim's HTTP
//! backend, and through the serverless MCP backend. Off, every answer is the bare ticket;
//! on, it carries `provisional`, and a red gate is reported once per claim. Gate readings
//! come from the Job store the way a monitor records them; no CI provider is contacted.
//! Synthetic tickets and commits only.
use super::*;
use hotsheet_jobs::{
    Acquire, ActiveStatus, CiTarget, CiWatchOptions, FinalStatus, JobStore, Outcome,
};
use hotsheet_mcp::{Backend, CoreBackend, HttpBackend, handle_message};
use hotsheet_model::{NoteKind, StartedPhase, Status, Ticket, Ulid};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::provisional::SpeculationPolicy;
use hotsheet_ticketing::{NewTicket, Settings, ops};

const NOW: &str = "2026-11-16T00:00:00Z";

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=s@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn request(method: &str, url: &str, body: Value) -> (u16, String) {
    let response = match ureq::request(method, url)
        .set("x-hotsheet-secret", SECRET)
        .set("content-type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => panic!("{error}"),
    };
    (response.status(), response.into_string().unwrap())
}

fn tool(backend: &dyn Backend, name: &str, arguments: Value) -> Value {
    let reply = handle_message(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name": name, "arguments": arguments}}),
        backend,
    )
    .unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn create(store: &FsStore, title: &str, blocked_by: Vec<Ulid>) -> Ticket {
    ops::create(
        store,
        Ulid::new(),
        "TST",
        NOW.into(),
        NewTicket {
            title: title.into(),
            blocked_by,
            ..Default::default()
        },
    )
    .unwrap()
}

fn judge_red(home: &std::path::Path, checkout: &str, job_id: &str, head: &str) {
    let store = JobStore::at(home);
    let (_, token) = store
        .acquire(
            checkout,
            job_id,
            Acquire {
                owner: "monitor:test".into(),
                status: ActiveStatus::Running,
                launch_id: None,
                external: None,
            },
        )
        .unwrap();
    let refs = [
        ("gate", "red"),
        ("run_id", "77"),
        ("run_attempt", "2"),
        ("run_head_sha", head),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    store
        .finish(
            checkout,
            job_id,
            &token,
            FinalStatus::Failed,
            Outcome {
                summary: None,
                refs,
            },
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hs2_xyf4yt_claim_answers_carry_provisional_prerequisites_on_every_surface() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("store");
    let host = state(&source, &home);
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    git(
        &project,
        &["commit", "-q", "--allow-empty", "-m", "synthetic base"],
    );
    let commit = git(&project, &["rev-parse", "HEAD"]);
    let checkout = CheckoutRegistry::new(home.join("checkouts.json"))
        .register(&project, None, None, vec![source.clone()])
        .unwrap();
    let store = FsStore::open(&source).unwrap();
    let mut prerequisite = create(&store, "Synthetic prerequisite", Vec::new());
    prerequisite.status = Status::Started;
    prerequisite.started_phase = Some(StartedPhase::FinalTesting);
    store.write_ticket(&prerequisite).unwrap();
    ops::add_note(
        &store,
        &prerequisite.id,
        Ulid::new(),
        NOW.into(),
        NoteKind::Regular,
        format!("Pushed.\n\nCI gate commit: {commit}\n"),
    )
    .unwrap();
    let http_dependent = create(&store, "HTTP dependent", vec![prerequisite.id]);
    let mcp_dependent = create(&store, "MCP dependent", vec![prerequisite.id]);
    let serverless_dependent = create(&store, "Serverless dependent", vec![prerequisite.id]);
    let jobs = JobStore::at(&home);
    let watch = hotsheet_jobs::ensure_ci_watch(
        &jobs,
        &checkout.id,
        &CiTarget::new("synthetic/repo", &commit, "CI", "main").unwrap(),
        &CiWatchOptions::default(),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app(host)).await.unwrap() });

    tokio::task::spawn_blocking(move || {
        let http = &http_dependent.slug;
        let checkout_base = format!("{base}/checkouts/{}", checkout.id);
        // Off: the default-store and checkout answers are the bare ticket, byte for byte
        // the same as before the field existed.
        let (status, claimed) = request(
            "POST",
            &format!("{base}/tickets/{http}/claim"),
            json!({"worker": "agent"}),
        );
        assert_eq!(status, 200, "{claimed}");
        assert!(!claimed.contains("provisional"), "{claimed}");
        let (status, renewed) = request(
            "POST",
            &format!("{checkout_base}/tickets/{http}/renew"),
            json!({"worker": "agent"}),
        );
        assert_eq!(status, 200, "{renewed}");
        assert!(!renewed.contains("provisional"), "{renewed}");

        SpeculationPolicy {
            enabled: true,
            base_branch: Some("main".into()),
            ..SpeculationPolicy::default()
        }
        .write(&Settings::for_project(&project))
        .unwrap();

        // Awaiting CI: both routes name the prerequisite and its commit on every renew.
        for path in [
            format!("{base}/tickets/{http}/renew"),
            format!("{checkout_base}/tickets/{http}/renew"),
        ] {
            let (status, body) = request("POST", &path, json!({"worker": "agent"}));
            assert_eq!(status, 200, "{body}");
            let body: Value = serde_json::from_str(&body).unwrap();
            let provisional = &body["provisional"];
            assert_eq!(provisional["integration"], "push-per-ticket", "{body}");
            assert_eq!(
                provisional["prerequisites"][0]["slug"],
                prerequisite.slug.as_str()
            );
            assert_eq!(
                provisional["prerequisites"][0]["base_commit"],
                commit.as_str()
            );
            assert_eq!(provisional["prerequisites"][0]["gate"]["result"], "pending");
            assert!(provisional.get("red_gates").is_none(), "{body}");
        }

        // MCP over HTTP: claim through the checkout, pending is named.
        let mcp = HttpBackend::new(&base, SECRET);
        let mcp_slug = mcp_dependent.slug.as_str();
        let claimed = tool(
            &mcp,
            "hotsheet_claim",
            json!({"id": mcp_slug, "worker": "mcp-agent", "checkout": checkout.id}),
        );
        assert_eq!(claimed["claimed_by"], "mcp-agent", "{claimed}");
        assert_eq!(
            claimed["provisional"]["prerequisites"][0]["base_commit"],
            commit.as_str()
        );

        // Red: each claim is warned once; a repeat renew carries no `provisional`.
        judge_red(&home, &checkout.id, &watch.id, &commit);
        let (_, red) = request(
            "POST",
            &format!("{base}/tickets/{http}/renew"),
            json!({"worker": "agent"}),
        );
        let red: Value = serde_json::from_str(&red).unwrap();
        assert_eq!(
            red["provisional"]["red_gates"][0]["gate"]["result"], "red",
            "{red}"
        );
        assert_eq!(red["provisional"]["red_gates"][0]["gate"]["run"], "77.2");
        assert!(red["provisional"].get("prerequisites").is_none(), "{red}");
        let (_, quiet) = request(
            "POST",
            &format!("{checkout_base}/tickets/{http}/renew"),
            json!({"worker": "agent"}),
        );
        assert!(!quiet.contains("provisional"), "{quiet}");
        let warned = tool(
            &mcp,
            "hotsheet_renew",
            json!({"id": mcp_slug, "worker": "mcp-agent", "checkout": checkout.id}),
        );
        assert_eq!(
            warned["provisional"]["red_gates"][0]["slug"],
            prerequisite.slug.as_str()
        );
        let quiet = tool(
            &mcp,
            "hotsheet_renew",
            json!({"id": mcp_slug, "worker": "mcp-agent", "checkout": checkout.id}),
        );
        assert!(quiet.get("provisional").is_none(), "{quiet}");

        // Serverless MCP: its own claim is warned once from the same machine record.
        let serverless = CoreBackend::open(&source)
            .unwrap()
            .with_checkout_registry(CheckoutRegistry::new(home.join("checkouts.json")));
        let slug = serverless_dependent.slug.as_str();
        let claimed = tool(
            &serverless,
            "hotsheet_claim",
            json!({"id": slug, "worker": "local"}),
        );
        assert_eq!(
            claimed["provisional"]["red_gates"][0]["gate_commit"],
            commit.as_str()
        );
        let quiet = tool(
            &serverless,
            "hotsheet_renew",
            json!({"id": slug, "worker": "local"}),
        );
        assert!(quiet.get("provisional").is_none(), "{quiet}");

        // Closed prerequisite: confirmed, the answers return to the bare ticket.
        let mut closed = store.read_ticket(&prerequisite.id).unwrap();
        closed.status = Status::Completed;
        store.write_ticket(&closed).unwrap();
        let (_, done) = request(
            "POST",
            &format!("{base}/tickets/{http}/renew"),
            json!({"worker": "agent"}),
        );
        assert!(!done.contains("provisional"), "{done}");
    })
    .await
    .unwrap();
}
