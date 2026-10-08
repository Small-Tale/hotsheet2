use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hotsheet_extsync::{
    GitHubConfig, GitHubProvider, GitHubTransport, GitLabConfig, GitLabProvider, HttpResponse,
    JiraConfig, JiraProvider,
};
use hotsheet_model::Timestamp;
use hotsheet_ticketing::{ProviderError, ProviderMutationTiming, ProviderPatch, TicketProvider};
use serde_json::{Value, json};

struct ControlledTransport {
    read_delay: Duration,
    write_delay: Duration,
    rate_limit_every: usize,
    reads: AtomicUsize,
}

impl GitHubTransport for ControlledTransport {
    fn request(
        &self,
        method: &str,
        url: &str,
        _headers: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let number = url.rsplit('/').next().unwrap().parse::<u64>().unwrap();
        std::thread::sleep(if method == "GET" {
            self.read_delay
        } else {
            self.write_delay
        });
        if method == "GET"
            && self.rate_limit_every > 0
            && self.reads.fetch_add(1, Ordering::SeqCst) % self.rate_limit_every == 0
        {
            return Ok(HttpResponse {
                status: 429,
                headers: HashMap::from([("retry-after".into(), "1".into())]),
                body: json!({"message":"controlled rate limit"}).to_string(),
            });
        }
        let title = body
            .and_then(|value| value.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("before");
        Ok(HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: json!({
                "number": number,
                "title": title,
                "body": "benchmark fixture",
                "state": "open",
                "state_reason": null,
                "html_url": format!("https://example.invalid/issues/{number}"),
                "created_at": "2026-10-08T00:00:00Z",
                "updated_at": "2026-10-08T00:01:00Z",
                "closed_at": null,
                "labels": [],
                "assignees": [],
                "pull_request": null
            })
            .to_string(),
        })
    }
}

fn distribution(values: &[f64]) -> (f64, f64) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| sorted[((sorted.len() - 1) as f64 * fraction).ceil() as usize];
    (percentile(0.5), percentile(0.95))
}

struct OtherProviderTransport {
    kind: &'static str,
}

impl GitHubTransport for OtherProviderTransport {
    fn request(
        &self,
        method: &str,
        url: &str,
        _headers: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        std::thread::sleep(Duration::from_millis(if method == "PUT" { 3 } else { 2 }));
        let issue = url
            .split('?')
            .next()
            .unwrap()
            .split('/')
            .rev()
            .find(|segment| segment.parse::<u64>().is_ok() || segment.starts_with("ENG-"))
            .unwrap_or("1");
        let title = body
            .and_then(|value| {
                value
                    .pointer("/fields/summary")
                    .or_else(|| value.get("title"))
            })
            .and_then(Value::as_str)
            .unwrap_or("before");
        let response = match self.kind {
            "gitlab" => json!({
                "iid":issue.parse::<u64>().unwrap(),"title":title,"description":"body",
                "state":"opened","web_url":format!("https://example.invalid/issues/{issue}"),
                "created_at":"2026-10-08T00:00:00Z","updated_at":"2026-10-08T00:01:00Z",
                "closed_at":null,"labels":["category:bug"],"assignees":[]
            }),
            "jira" if url.contains("/comment?") => json!({"comments":[]}),
            "jira" if method == "PUT" => json!({}),
            "jira" => json!({
                "key":issue,"fields":{"summary":title,"description":null,
                "status":{"statusCategory":{"key":"indeterminate"}},
                "priority":{"name":"High"},"issuetype":{"name":"Bug"},
                "labels":[],"assignee":null,"created":"2026-10-08T00:00:00Z",
                "updated":"2026-10-08T00:01:00Z","resolutiondate":null}
            }),
            _ => unreachable!(),
        };
        Ok(HttpResponse {
            status: if self.kind == "jira" && method == "PUT" {
                204
            } else {
                200
            },
            headers: HashMap::new(),
            body: response.to_string(),
        })
    }
}

#[test]
fn controlled_gitlab_and_jira_latency_distributions() {
    for kind in ["gitlab", "jira"] {
        let transport = Arc::new(OtherProviderTransport { kind });
        let provider: Box<dyn TicketProvider> = if kind == "gitlab" {
            Box::new(GitLabProvider::new(
                GitLabConfig {
                    connection_id: "gitlab-bench".into(),
                    project: "acme/bench".into(),
                    api_base: "https://example.invalid".into(),
                    token: "redacted-token".into(),
                    default: false,
                },
                transport,
            ))
        } else {
            Box::new(JiraProvider::new(
                JiraConfig {
                    connection_id: "jira-bench".into(),
                    project_key: "ENG".into(),
                    base_url: "https://example.invalid".into(),
                    email: "bench@example.invalid".into(),
                    token: "redacted-token".into(),
                    default: false,
                },
                transport,
            ))
        };
        for count in [1, 20, 100] {
            let batch_started = Instant::now();
            let mut totals = Vec::with_capacity(count);
            let mut reads = Vec::with_capacity(count);
            let mut writes = Vec::with_capacity(count);
            let mut acknowledgements = Vec::with_capacity(count);
            for number in 1..=count {
                let id = if kind == "jira" {
                    format!("ENG-{number}")
                } else {
                    number.to_string()
                };
                let started = Instant::now();
                let (_, timing) = provider
                    .update_timed(
                        &id,
                        Timestamp::new("2026-10-08T00:02:00Z"),
                        ProviderPatch {
                            title: Some("measured".into()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                totals.push(started.elapsed().as_secs_f64() * 1000.0);
                reads.push(timing.remote_read.as_secs_f64() * 1000.0);
                writes.push(timing.remote_write.as_secs_f64() * 1000.0);
                acknowledgements.push(timing.acknowledgement.as_secs_f64() * 1000.0);
                assert!(timing.remote_read >= Duration::from_millis(2));
                assert!(timing.remote_write >= Duration::from_millis(3));
                if kind == "jira" {
                    assert!(timing.acknowledgement >= Duration::from_millis(4));
                }
            }
            println!(
                "provider={kind} count={count} batch_ms={:.1} ticket_p50_p95_ms={:?} read_p50_p95_ms={:?} write_p50_p95_ms={:?} ack_p50_p95_ms={:?}",
                batch_started.elapsed().as_secs_f64() * 1000.0,
                distribution(&totals),
                distribution(&reads),
                distribution(&writes),
                distribution(&acknowledgements)
            );
        }
    }
}

#[test]
fn controlled_provider_latency_distributions_for_one_twenty_and_one_hundred() {
    for count in [1, 20, 100] {
        let transport = Arc::new(ControlledTransport {
            read_delay: Duration::from_millis(2),
            write_delay: Duration::from_millis(3),
            rate_limit_every: 0,
            reads: AtomicUsize::new(1),
        });
        let mut config = GitHubConfig::new("github-bench", "acme/bench", "redacted-token");
        config.api_base = "https://example.invalid".into();
        let provider = GitHubProvider::new(config, transport);
        let batch_started = Instant::now();
        let mut totals = Vec::with_capacity(count);
        let mut reads = Vec::with_capacity(count);
        let mut writes = Vec::with_capacity(count);
        for number in 1..=count {
            let started = Instant::now();
            let (_, timing) = provider
                .update_timed(
                    &number.to_string(),
                    Timestamp::new("2026-10-08T00:02:00Z"),
                    ProviderPatch {
                        title: Some("measured".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
            totals.push(started.elapsed().as_secs_f64() * 1000.0);
            reads.push(timing.remote_read.as_secs_f64() * 1000.0);
            writes.push(timing.remote_write.as_secs_f64() * 1000.0);
            assert!(timing.remote_read >= Duration::from_millis(2));
            assert!(timing.remote_write >= Duration::from_millis(3));
            assert_eq!(timing.queue_wait, Duration::ZERO);
        }
        let total = distribution(&totals);
        let read = distribution(&reads);
        let write = distribution(&writes);
        println!(
            "count={count} batch_ms={:.1} ticket_p50_p95_ms={total:?} read_p50_p95_ms={read:?} write_p50_p95_ms={write:?}",
            batch_started.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!(totals.len(), count);
    }

    let transport = Arc::new(ControlledTransport {
        read_delay: Duration::from_millis(2),
        write_delay: Duration::from_millis(3),
        rate_limit_every: 20,
        reads: AtomicUsize::new(1),
    });
    let provider = GitHubProvider::new(
        GitHubConfig::new("github-bench", "acme/bench", "redacted-token"),
        transport,
    );
    let mut limited = 0;
    for number in 1..=100 {
        let result = provider.update_timed(
            &number.to_string(),
            Timestamp::new("2026-10-08T00:02:00Z"),
            ProviderPatch::default(),
        );
        if matches!(result, Err(ProviderError::RateLimited { .. })) {
            limited += 1;
        } else {
            result.unwrap();
        }
    }
    assert_eq!(limited, 5);
    println!("count=100 controlled_rate_limits={limited}");
}

#[test]
#[ignore = "opt-in live mutation of a designated GitHub benchmark issue"]
fn opt_in_real_github_mutation_latency() {
    assert_eq!(
        std::env::var("HOTSHEET_REAL_PROVIDER_BENCH").as_deref(),
        Ok("1"),
        "set HOTSHEET_REAL_PROVIDER_BENCH=1 to run this ignored test"
    );
    let repository = std::env::var("HOTSHEET_BENCH_GITHUB_REPOSITORY").unwrap();
    let token = std::env::var("HOTSHEET_BENCH_GITHUB_TOKEN").unwrap();
    let issue = std::env::var("HOTSHEET_BENCH_GITHUB_ISSUE").unwrap();
    let provider = GitHubProvider::live(GitHubConfig::new("github-bench", repository, token));
    let mut current = provider.get(&issue).unwrap();
    let mut samples: Vec<ProviderMutationTiming> = Vec::new();
    for _ in 0..3 {
        let (ticket, timing) = provider
            .update_timed(
                &issue,
                Timestamp::new("2026-10-08T00:02:00Z"),
                ProviderPatch {
                    expected_token: current.concurrency_token.clone(),
                    title: Some(current.title.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        current = ticket;
        samples.push(timing);
    }
    for (index, timing) in samples.iter().enumerate() {
        println!(
            "real_sample={} read_ms={:.1} token_ms={:.1} write_ms={:.1} ack_ms={:.1}",
            index + 1,
            timing.remote_read.as_secs_f64() * 1000.0,
            timing.token_check.as_secs_f64() * 1000.0,
            timing.remote_write.as_secs_f64() * 1000.0,
            timing.acknowledgement.as_secs_f64() * 1000.0,
        );
    }
}
