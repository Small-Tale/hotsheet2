use assert_cmd::Command;
use hotsheet_model::{AiFeedbackRating, AttachmentActorRole, NoteActor, Timestamp, Ulid};
use hotsheet_ticketing::{
    FsStore, ProviderConfigRegistry, ProviderConnection, StoreMetadata, git_connection_id, ops,
};

#[test]
fn headless_feedback_review_is_private_opt_in_and_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join("tickets");
    let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
    let id = Ulid::new();
    ops::create(
        &store,
        id,
        "HS",
        Timestamp::new("2026-10-01T00:00:00Z"),
        ops::NewTicket::default(),
    )
    .unwrap();
    ops::rate_ai_content(
        &store,
        &id,
        Timestamp::new("2026-10-02T00:00:00Z"),
        "activity:run-1",
        Some(AiFeedbackRating::NotHelpful),
        Some("Explain the reason".into()),
        Some(NoteActor {
            role: AttachmentActorRole::Human,
            id: Some("reviewer-1".into()),
        }),
    )
    .unwrap();
    let connection = git_connection_id(&store);
    let run = |args: &[&str]| {
        Command::cargo_bin("hotsheet-cli")
            .unwrap()
            .env("HOTSHEET_HOME", &home)
            .args(["-C", root.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    };

    let first = run(&["feedback-synthesis", "prepare", "--connection", &connection]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let output = String::from_utf8(first.stdout).unwrap();
    assert!(output.contains("Review draft:"));
    let path = output.trim().strip_prefix("Review draft: ").unwrap();
    assert!(!std::path::Path::new(path).starts_with(&root));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(std::path::Path::new(path).parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let draft = std::fs::read_to_string(path).unwrap();
    assert!(draft.contains("Explain the reason") || draft.contains("explain the reason"));
    assert!(draft.contains("below the independent-example threshold"));
    assert!(
        run(&[
            "feedback-synthesis",
            "accept",
            "--reviewed",
            "--actor-role",
            "ai"
        ])
        .status
        .code()
            != Some(0)
    );
    let accepted = run(&[
        "feedback-synthesis",
        "accept",
        "--reviewed",
        "--actor-role",
        "human",
    ]);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let rerun = run(&["feedback-synthesis", "prepare", "--connection", &connection]);
    assert!(rerun.status.success());
    assert!(String::from_utf8_lossy(&rerun.stdout).contains("No new AI feedback"));
    let unavailable = run(&["feedback-synthesis", "prepare", "--connection", "unknown"]);
    assert!(!unavailable.status.success());
    assert!(
        String::from_utf8_lossy(&unavailable.stderr)
            .contains("no selected provider could supply AI feedback")
    );
    assert!(String::from_utf8_lossy(&unavailable.stderr).contains("unknown"));
    ProviderConfigRegistry::new(root.join("providers.json"))
        .save(&[ProviderConnection {
            id: "github-test".into(),
            provider: "github".into(),
            locator: "owner/repository".into(),
            name: None,
            default: false,
            settings: serde_json::json!({}),
            disabled: false,
        }])
        .unwrap();
    let unsupported = run(&[
        "feedback-synthesis",
        "prepare",
        "--connection",
        "github-test",
    ]);
    assert!(!unsupported.status.success());
    assert!(
        String::from_utf8_lossy(&unsupported.stderr)
            .contains("does not support AI feedback retrieval")
    );
    assert!(!root.join("draft.md").exists());
}
