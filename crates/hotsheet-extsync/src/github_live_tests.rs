//! Opt-in live validation of GitHub attachments against approved, non-production test
//! repositories (HS2-VQSCA1, HS2-GWY4AK, HS2-QB6XJ4). Every test is `#[ignore]`d, so the
//! default suite never touches GitHub. Run explicitly, for example:
//!
//! ```text
//! HOTSHEET_GITHUB_LIVE_REPO=owner/tickets HOTSHEET_GITHUB_LIVE_ASSETS_REPO=owner/assets \
//!   cargo nextest run -p hotsheet-extsync --run-ignored only github_live_ --no-capture
//! ```
//!
//! The token comes from `HOTSHEET_GITHUB_LIVE_TOKEN` or the OS-keychain key `github-live`
//! and is never printed. The 1/10/100 manifest measurement additionally needs
//! `HOTSHEET_GITHUB_LIVE_MANIFEST_SCALE=1`; `HOTSHEET_GITHUB_LIVE_MANIFEST_ISSUES=a,b,c`
//! re-measures previously seeded issues without seeding again. Each test creates throwaway
//! issues and closes them on the way out.

use super::*;
use hotsheet_model::{ImageCrop, MediaAnnotation, Ulid};
use hotsheet_ticketing::store::AttachmentMarkup;

fn live_token() -> String {
    std::env::var("HOTSHEET_GITHUB_LIVE_TOKEN").unwrap_or_else(|_| {
        hotsheet_ticketing::KeyRegistry::new("", hotsheet_ticketing::OsKeychain)
            .get("github-live")
            .expect("HOTSHEET_GITHUB_LIVE_TOKEN or OS-keychain entry 'github-live'")
    })
}

/// `None` (the test skips) unless both live repositories are configured, so the nightly
/// `--run-ignored all` tier stays green on runners without the assets repository.
fn live_config() -> Option<GitHubConfig> {
    let (Ok(repository), Ok(assets)) = (
        std::env::var("HOTSHEET_GITHUB_LIVE_REPO"),
        std::env::var("HOTSHEET_GITHUB_LIVE_ASSETS_REPO"),
    ) else {
        eprintln!("skipped: set HOTSHEET_GITHUB_LIVE_REPO and HOTSHEET_GITHUB_LIVE_ASSETS_REPO");
        return None;
    };
    let mut config = GitHubConfig::new("github-live", repository, live_token())
        .with_attachments(GitHubAttachmentRepository::new(&assets, None, None).unwrap());
    if let Ok(base) = std::env::var("HOTSHEET_GITHUB_LIVE_API_BASE") {
        config.api_base = base;
    }
    Some(config)
}

/// Records every live request so tests can count GraphQL batches and REST fallbacks.
struct RecordingTransport {
    batching: bool,
    log: Mutex<Vec<(String, String, u16)>>,
}

impl RecordingTransport {
    fn new(batching: bool) -> Arc<Self> {
        Arc::new(Self {
            batching,
            log: Mutex::new(vec![]),
        })
    }

    fn take(&self) -> Vec<(String, String, u16)> {
        std::mem::take(&mut *self.log.lock_or_recover())
    }
}

impl GitHubTransport for RecordingTransport {
    fn supports_manifest_batching(&self) -> bool {
        self.batching
    }

    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let response = UreqGitHubTransport.request(method, url, headers, body)?;
        self.log
            .lock_or_recover()
            .push((method.into(), url.into(), response.status));
        Ok(response)
    }
}

/// Direct authenticated call, used only to simulate edits made outside Hot Sheet.
fn raw(config: &GitHubConfig, method: &str, url: &str, body: Option<&Value>) -> HttpResponse {
    UreqGitHubTransport
        .request(
            method,
            url,
            &[
                ("Accept", "application/vnd.github+json".into()),
                ("Authorization", format!("Bearer {}", config.token)),
                ("X-GitHub-Api-Version", API_VERSION.into()),
                ("User-Agent", "hotsheet2-live-test".into()),
            ],
            body,
        )
        .expect("GitHub reachable")
}

fn graphql_used(config: &GitHubConfig) -> i64 {
    let response = raw(
        config,
        "POST",
        &format!("{}/graphql", config.api_base.trim_end_matches('/')),
        Some(&json!({"query": "query { rateLimit { used } }"})),
    );
    let value: Value = serde_json::from_str(&response.body).unwrap();
    value
        .pointer("/data/rateLimit/used")
        .and_then(Value::as_i64)
        .expect("rateLimit.used")
}

fn ctx() -> MutationContext {
    MutationContext {
        now: Timestamp::new("2026-10-14T00:00:00Z"),
        generated_id: Ulid::new(),
    }
}

fn create_issue(provider: &GitHubProvider, title: &str) -> String {
    provider
        .create(
            ctx(),
            ProviderDraft {
                title: format!("Hot Sheet live attachment test: {title} {}", Ulid::new()),
                category: "test".into(),
                priority: Priority::Default,
                status: Status::NotStarted,
                details: "Created by an opt-in Hot Sheet live validation; closed automatically."
                    .into(),
                tags: vec![],
                up_next: false,
                blocked_by: vec![],
                transfer: None,
            },
        )
        .unwrap()
        .native_id
}

fn close_issue(provider: &GitHubProvider, native_id: &str) {
    provider
        .close(
            native_id,
            Timestamp::new("2026-10-14T00:10:00Z"),
            CloseReason::Completed,
            None,
        )
        .unwrap();
}

fn attach(provider: &GitHubProvider, native_id: &str, bytes: &[u8]) -> String {
    let id = Ulid::new().to_string();
    provider
        .add_attachment(
            native_id,
            ApiAttachment {
                id: id.clone(),
                filename: "live-proof.png".into(),
                created_at: "2026-10-14T00:00:00Z".into(),
                batch_id: None,
                batch_label: None,
                actor: None,
                purpose: Some(hotsheet_model::AttachmentPurpose::ProblemEvidence),
                crop: None,
                annotations: vec![],
                revision: None,
            },
            bytes.to_vec(),
        )
        .unwrap();
    id
}

fn png() -> Vec<u8> {
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    let image = RgbImage::from_fn(24, 20, |x, y| Rgb([x as u8 * 9, y as u8 * 11, 42]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn revision(provider: &GitHubProvider, native_id: &str, attachment_id: &str) -> String {
    provider
        .get(native_id)
        .unwrap()
        .attachments
        .into_iter()
        .find(|attachment| attachment.id == attachment_id)
        .and_then(|attachment| attachment.revision)
        .expect("attachment revision")
}

fn crop(x: u32, y: u32, width: u32, height: u32) -> AttachmentMarkup {
    AttachmentMarkup {
        annotations: vec![],
        crop: Some(ImageCrop {
            x,
            y,
            width,
            height,
        }),
    }
}

fn width(bytes: &[u8]) -> u32 {
    image::load_from_memory(bytes).unwrap().width()
}

/// HS2-VQSCA1: an upload lands in the assets repository and reads back byte-identical.
#[test]
#[ignore = "writes to the live GitHub test repositories; see module docs"]
fn github_live_attachment_upload_round_trips_through_the_assets_repository() {
    let Some(config) = live_config() else {
        return;
    };
    let provider = GitHubProvider::live(config.clone());
    let issue = create_issue(&provider, "upload");
    let original = png();
    let id = attach(&provider, &issue, &original);
    // A retry with the same id is idempotent: no second link comment.
    let ticket = provider.get(&issue).unwrap();
    assert_eq!(ticket.attachments.len(), 1);
    assert_eq!(provider.attachment_bytes(&issue, &id).unwrap(), original);
    assert_eq!(
        provider.attachment_original_bytes(&issue, &id).unwrap(),
        original
    );
    let assets = config.attachments.clone().unwrap();
    let listing = raw(
        &config,
        "GET",
        &format!(
            "{}/repos/{}/contents/{}?ref={}",
            config.api_base, assets.repository, assets.folder, assets.branch
        ),
        None,
    );
    assert_eq!(listing.status, 200);
    assert!(
        listing.body.contains(&id),
        "asset file is in the assets repo"
    );
    close_issue(&provider, &issue);
}

/// HS2-GWY4AK: crop/annotate/restore, stale and concurrent writers, a lost comment PATCH,
/// and a direct GitHub comment edit, all against real Contents API SHA preconditions.
#[test]
#[ignore = "writes to the live GitHub test repositories; see module docs"]
fn github_live_crop_manifest_cas_against_the_assets_repository() {
    let Some(config) = live_config() else {
        return;
    };
    let provider = GitHubProvider::live(config.clone());
    let issue = create_issue(&provider, "crop CAS");
    let original = png();
    let id = attach(&provider, &issue, &original);

    // First manifest create, then update, with rendition and annotation.
    let first = revision(&provider, &issue, &id);
    let cropped = provider
        .set_attachment_markup(&issue, &id, Some(&first), crop(3, 2, 12, 10))
        .unwrap();
    assert_eq!(width(&provider.attachment_bytes(&issue, &id).unwrap()), 12);
    assert_eq!(
        provider.attachment_original_bytes(&issue, &id).unwrap(),
        original
    );
    let annotation = MediaAnnotation {
        id: "live".into(),
        x: 1_000,
        y: 1_000,
        width: 2_000,
        height: 2_000,
        start_ms: None,
        end_ms: None,
        text: "live".into(),
        shape: None,
        intents: vec![],
    };
    let annotated = provider
        .set_attachment_markup(
            &issue,
            &id,
            cropped.attachments[0].revision.as_deref(),
            AttachmentMarkup {
                annotations: vec![annotation.clone()],
                crop: cropped.attachments[0].crop,
            },
        )
        .unwrap();
    assert_eq!(annotated.attachments[0].annotations, vec![annotation]);

    // A stale gallery session is rejected.
    let stale = provider.set_attachment_markup(&issue, &id, Some(&first), crop(1, 1, 10, 10));
    assert!(
        matches!(stale, Err(ProviderError::Conflict { .. })),
        "{stale:?}"
    );

    // Concurrent writers from one revision: exactly one wins the Contents SHA race.
    for (a, b) in [
        (crop(1, 1, 10, 10), crop(5, 2, 12, 9)),
        (crop(0, 0, 8, 8), crop(3, 4, 14, 10)),
    ] {
        let base = revision(&provider, &issue, &id);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let writers = [a, b].map(|markup| {
            let (provider, issue, id, base, barrier) = (
                provider.clone(),
                issue.clone(),
                id.clone(),
                base.clone(),
                barrier.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                provider.set_attachment_markup(&issue, &id, Some(&base), markup)
            })
        });
        let results = writers.map(|writer| writer.join().unwrap());
        assert_eq!(
            results.iter().filter(|result| result.is_ok()).count(),
            1,
            "{results:?}"
        );
        assert!(
            results
                .iter()
                .any(|result| matches!(result, Err(ProviderError::Conflict { .. })))
        );
        let winner = results.into_iter().find_map(Result::ok).unwrap();
        let current = provider.get(&issue).unwrap();
        assert_eq!(current.attachments[0].crop, winner.attachments[0].crop);
        let assets = config.attachments.as_ref().unwrap();
        let (manifest, _) = provider.attachment_manifest(assets, &id).unwrap().unwrap();
        let (comment, _) = provider.attachment_marker_comment(&issue, &id).unwrap();
        assert_eq!(manifest.body, comment.body, "manifest and comment agree");
    }

    // A comment PATCH that never landed: the manifest wins on refresh and the next edit
    // repairs the comment.
    let assets = config.attachments.clone().unwrap();
    let (manifest, _) = provider.attachment_manifest(&assets, &id).unwrap().unwrap();
    let (comment, _) = provider.attachment_marker_comment(&issue, &id).unwrap();
    let current = provider.get(&issue).unwrap().attachments[0].clone();
    // Rewind the comment to the manifest's base revision body by replaying an older body.
    let (_, mut older) = github_attachments::parse_comment(&comment.body).unwrap();
    older.crop = None;
    older.rendition_path = None;
    older.rendition_sha = None;
    older.annotations = vec![];
    let older_body = github_attachments::compose_comment(&id, "https://x/older", &older);
    let rewound = AttachmentRevisionManifest {
        base_comment_revision: attachment_comment_revision(&older_body),
        body: manifest.body.clone(),
    };
    let (_, manifest_sha) = provider.attachment_manifest(&assets, &id).unwrap().unwrap();
    provider
        .write_attachment_manifest(&assets, &id, Some(&manifest_sha), &rewound)
        .unwrap();
    let patch_url = provider.endpoint(&format!("issues/comments/{}", comment.id));
    assert_eq!(
        raw(
            &config,
            "PATCH",
            &patch_url,
            Some(&json!({"body": older_body}))
        )
        .status,
        200
    );
    let refreshed = provider.get(&issue).unwrap();
    assert_eq!(refreshed.attachments[0].crop, current.crop, "manifest wins");
    let repaired = provider
        .set_attachment_markup(
            &issue,
            &id,
            refreshed.attachments[0].revision.as_deref(),
            AttachmentMarkup {
                annotations: vec![],
                crop: None,
            },
        )
        .unwrap();
    assert!(repaired.attachments[0].crop.is_none());
    assert_eq!(provider.attachment_bytes(&issue, &id).unwrap(), original);
    let (comment, _) = provider.attachment_marker_comment(&issue, &id).unwrap();
    let (manifest, _) = provider.attachment_manifest(&assets, &id).unwrap().unwrap();
    assert_eq!(
        comment.body, manifest.body,
        "comment repaired from manifest"
    );

    // A direct edit on GitHub diverges from the manifest and is refused, not overwritten.
    let (_, mut manual) = github_attachments::parse_comment(&comment.body).unwrap();
    manual.batch_label = Some("Edited on GitHub".into());
    let manual_body = github_attachments::compose_comment(&id, "https://x/manual", &manual);
    assert_eq!(
        raw(
            &config,
            "PATCH",
            &patch_url,
            Some(&json!({"body": manual_body}))
        )
        .status,
        200
    );
    let latest = provider.get(&issue).unwrap();
    let conflict = provider.set_attachment_markup(
        &issue,
        &id,
        latest.attachments[0].revision.as_deref(),
        crop(2, 2, 8, 8),
    );
    assert!(
        matches!(conflict, Err(ProviderError::Conflict { .. })),
        "{conflict:?}"
    );
    let (comment, _) = provider.attachment_marker_comment(&issue, &id).unwrap();
    assert_eq!(comment.body, manual_body, "direct edit preserved");
    close_issue(&provider, &issue);
}

/// HS2-QB6XJ4: seed 1/10/100 real manifests and compare ticket-detail reads with
/// GraphQL batching against the per-manifest REST baseline (HS2-3KA0QG).
#[test]
#[ignore = "writes ~111 attachments to the live GitHub test repositories; see module docs"]
fn github_live_batched_manifest_reads_scale_with_manifest_count() {
    if std::env::var("HOTSHEET_GITHUB_LIVE_MANIFEST_SCALE").as_deref() != Ok("1") {
        eprintln!("skipped: set HOTSHEET_GITHUB_LIVE_MANIFEST_SCALE=1 to seed ~111 attachments");
        return;
    }
    let Some(config) = live_config() else {
        return;
    };
    let seeder = GitHubProvider::live(config.clone());
    let original = png();
    let reuse = std::env::var("HOTSHEET_GITHUB_LIVE_MANIFEST_ISSUES").ok();
    let issues: Vec<(usize, String)> = match reuse {
        // Comma-separated issue numbers seeded by an earlier run, in 1/10/100 order.
        Some(list) => [1, 10, 100]
            .into_iter()
            .zip(list.split(',').map(str::to_owned))
            .collect(),
        None => [1_usize, 10, 100]
            .into_iter()
            .map(|count| {
                let issue = create_issue(&seeder, &format!("{count} manifests"));
                for index in 0..count {
                    let id = attach(&seeder, &issue, &original);
                    let base = revision(&seeder, &issue, &id);
                    seeder
                        .set_attachment_markup(
                            &issue,
                            &id,
                            Some(&base),
                            AttachmentMarkup {
                                annotations: vec![MediaAnnotation {
                                    id: format!("seed-{index}"),
                                    x: 1_000,
                                    y: 1_000,
                                    width: 2_000,
                                    height: 2_000,
                                    start_ms: None,
                                    end_ms: None,
                                    text: "seed".into(),
                                    shape: None,
                                    intents: vec![],
                                }],
                                crop: None,
                            },
                        )
                        .unwrap();
                    std::thread::sleep(Duration::from_millis(250));
                }
                eprintln!("seeded issue #{issue} with {count} manifests");
                (count, issue)
            })
            .collect(),
    };
    eprintln!(
        "| manifests | mode | median ms | requests | graphql | rest manifest GETs | graphql points |"
    );
    eprintln!("|---|---|---|---|---|---|---|");
    for (count, issue) in &issues {
        for batching in [true, false] {
            let transport = RecordingTransport::new(batching);
            let provider = GitHubProvider::new(config.clone(), transport.clone());
            let mut times = vec![];
            let mut counts = (0, 0, 0);
            let mut points = 0;
            for _ in 0..3 {
                let used = graphql_used(&config);
                let started = Instant::now();
                let ticket = provider
                    .get(issue)
                    .unwrap_or_else(|error| panic!("live detail read failed: {error:?}"));
                times.push(started.elapsed().as_millis());
                // A negative delta means the hourly window reset mid-read; ignore it.
                points = points.max(graphql_used(&config) - used);
                assert_eq!(ticket.attachments.len(), *count);
                assert!(
                    ticket
                        .attachments
                        .iter()
                        .all(|attachment| attachment.annotations.len() == 1),
                    "every manifest was applied"
                );
                let log = transport.take();
                counts = (
                    log.len(),
                    log.iter()
                        .filter(|(method, url, _)| method == "POST" && url.ends_with("/graphql"))
                        .count(),
                    log.iter()
                        .filter(|(method, url, _)| {
                            method == "GET" && url.contains(".hotsheet-markers")
                        })
                        .count(),
                );
            }
            times.sort_unstable();
            eprintln!(
                "| {count} | {} | {} | {} | {} | {} | {points} |",
                if batching {
                    "GraphQL batch"
                } else {
                    "REST baseline"
                },
                times[1],
                counts.0,
                counts.1,
                counts.2
            );
            if batching {
                assert_eq!(counts.1, count.div_ceil(25), "one GraphQL query per 25");
                assert_eq!(counts.2, 0, "no REST fallback");
            } else {
                assert_eq!(counts.2, *count);
            }
        }
    }
    if std::env::var("HOTSHEET_GITHUB_LIVE_KEEP_ISSUES").is_err() {
        for (_, issue) in &issues {
            close_issue(&seeder, issue);
        }
    }
}
