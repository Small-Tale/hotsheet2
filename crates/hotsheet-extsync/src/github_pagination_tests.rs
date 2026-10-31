//! HS2-TAG2ZP: independent provider/device flows through fake and real transports.

use super::*;
use crate::GitHubDeviceClient;
use std::collections::VecDeque;

struct Pages {
    responses: Mutex<VecDeque<HttpResponse>>,
    urls: Mutex<Vec<String>>,
}

impl GitHubTransport for Pages {
    fn request(
        &self,
        _: &str,
        url: &str,
        _: &[(&str, String)],
        _: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        self.urls.lock_or_recover().push(url.into());
        self.responses
            .lock_or_recover()
            .pop_front()
            .ok_or_else(|| "unexpected pagination request".into())
    }
}

fn page(body: Value, next: Option<&str>) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: next
            .map(|url| HashMap::from([("link".into(), format!("<{url}>; rel=\"next\""))]))
            .unwrap_or_default(),
        body: body.to_string(),
    }
}

fn pages(responses: Vec<HttpResponse>) -> Arc<Pages> {
    Arc::new(Pages {
        responses: Mutex::new(responses.into()),
        urls: Mutex::new(Vec::new()),
    })
}

fn provider(transport: Arc<Pages>) -> GitHubProvider {
    let mut config = GitHubConfig::new("synthetic", "synthetic/issues", "synthetic-token");
    config.api_base = "https://ghe.test/api/v3".into();
    GitHubProvider::new(config, transport)
}

#[test]
fn hs2_tag2zp_both_owners_reject_links_before_another_authenticated_request() {
    for next in [
        "https://foreign.test/api/v3/issues?page=2",
        "http://ghe.test/api/v3/issues?page=2",
        "https://ghe.test:443/api/v3/issues?page=2",
        "https://ghe.test.evil.test/api/v3/issues?page=2",
        "https://synthetic@ghe.test/api/v3/issues?page=2",
        "https://ghe.test/api/v3/issues?page=2#fragment",
        "https://ghe.test\\@foreign.test/api/v3/issues?page=2",
        "/api/v3/issues?page=2",
        "?page=2",
        "//ghe.test/api/v3/issues?page=2",
        "file://ghe.test/api/v3/issues?page=2",
    ] {
        let transport = pages(vec![page(json!({"installations": []}), Some(next))]);
        assert!(
            GitHubDeviceClient::new("synthetic", "https://ghe.test", transport.clone())
                .repository_access("synthetic-token")
                .is_err(),
            "device {next}"
        );
        assert_eq!(transport.urls.lock_or_recover().len(), 1);
        let transport = pages(vec![page(json!([]), Some(next))]);
        let provider = provider(transport.clone());
        assert!(
            provider
                .query_page(&TicketQuery::default(), None, 100)
                .is_err(),
            "provider {next}"
        );
        assert_eq!(transport.urls.lock_or_recover().len(), 1);
        assert!(
            provider
                .query_page(&TicketQuery::default(), Some(next), 100)
                .is_err()
        );
        assert_eq!(transport.urls.lock_or_recover().len(), 1);
    }
}

#[test]
fn hs2_tag2zp_enterprise_cursor_prefix_and_origin_casing_are_preserved() {
    for next in [
        "https://ghe.test/repos/synthetic/issues/issues?page=2",
        "https://ghe.test/api/v30/issues?page=2",
        "https://ghe.test/api/v3/../issues?page=2",
        "https://ghe.test/api/v3/%2e%2e/issues?page=2",
    ] {
        let transport = pages(vec![]);
        assert!(
            provider(transport.clone())
                .query_page(&TicketQuery::default(), Some(next), 100)
                .is_err()
        );
        assert!(transport.urls.lock_or_recover().is_empty());
    }
    let next = "HTTPS://GHE.TEST/api/v3/repos/synthetic/issues/issues?page=2";
    let transport = pages(vec![page(json!([]), Some(next)), page(json!([]), None)]);
    let provider = provider(transport.clone());
    let first = provider
        .query_page(&TicketQuery::default(), None, 100)
        .unwrap();
    assert_eq!(
        first.next_cursor.as_deref(),
        Some("https://ghe.test/api/v3/repos/synthetic/issues/issues?page=2")
    );
    assert!(
        provider
            .query_page(&TicketQuery::default(), first.next_cursor.as_deref(), 100)
            .unwrap()
            .next_cursor
            .is_none()
    );
    assert_eq!(transport.urls.lock_or_recover().len(), 2);
}

// HS2-TAG2ZP: keep the original encoded resource plus explicit ordinary
// percent-space/UTF-8 path and query cells. Repository identity stays unchanged.
const ENCODED_PAGE_CASES: [(&str, &str); 3] = [
    ("/api/v%33", "page=2&cursor=a%2Fb%2Bc&Case=Keep"),
    (
        "/api/space%20segment/v3",
        "page=2&cursor=space%20value&Case=Keep",
    ),
    (
        "/api/%E2%9C%93/v3",
        "page=2&cursor=%E6%97%A5%20%E2%9C%93&Case=Keep",
    ),
];

#[test]
fn hs2_tag2zp_encoded_enterprise_resource_preserves_supplied_and_returned_cursor_bytes() {
    for (api_path, query) in ENCODED_PAGE_CASES {
        let base = format!("https://ghe.test{api_path}");
        let resource = format!("{base}/repos/synthetic%2Dorg/repo%2ename/issues");
        let next = format!("{resource}?{query}");
        let transport = pages(vec![page(json!([]), Some(&next)), page(json!([]), None)]);
        let mut config = GitHubConfig::new(
            "synthetic",
            "synthetic%2Dorg/repo%2ename",
            "synthetic-token",
        );
        config.api_base = base.clone();
        let provider = GitHubProvider::new(config, transport.clone());
        let first = provider
            .query_page(&TicketQuery::default(), None, 100)
            .unwrap();
        assert_eq!(first.next_cursor.as_deref(), Some(next.as_str()));
        provider
            .query_page(&TicketQuery::default(), Some(&next), 100)
            .unwrap();
        assert_eq!(transport.urls.lock_or_recover()[1], next);

        // Encodings within a segment and in a query do not admit path separators
        // or dot segments. Rejected supplied cursors must issue no further request.
        for path in [
            "repos/synthetic%2Dorg/repo%2Fname/issues",
            "repos/synthetic%2Dorg/%2e%2e/issues",
            "repos/synthetic%2Dorg/repo%5cname/issues",
        ] {
            assert!(
                provider
                    .query_page(
                        &TicketQuery::default(),
                        Some(&format!("{base}/{path}?page=2")),
                        100,
                    )
                    .is_err()
            );
        }
        assert_eq!(transport.urls.lock_or_recover().len(), 2);
    }
}

#[test]
fn hs2_tag2zp_real_ureq_preserves_encoded_resource_and_query_request_target() {
    for (api_path, query) in ENCODED_PAGE_CASES {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}{api_path}", listener.local_addr().unwrap());
        let request_target = format!("{api_path}/repos/synthetic%2Dorg/repo%2ename/issues?{query}");
        let next = format!("{base}/repos/synthetic%2Dorg/repo%2ename/issues?{query}");
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline, "request missing");
                        std::thread::yield_now();
                    }
                    Err(error) => panic!("selected listener failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            // Bound the complete synthetic header block; never retain bearer headers.
            let mut reader =
                BufReader::new(std::io::Read::take(stream.try_clone().unwrap(), 16384));
            let mut first = String::new();
            assert_ne!(reader.read_line(&mut first).unwrap(), 0);
            let mut line = String::new();
            loop {
                line.clear();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]"
            )
            .unwrap();
            first
        });
        let mut config = GitHubConfig::new(
            "synthetic",
            "synthetic%2Dorg/repo%2ename",
            "synthetic-token",
        );
        config.api_base = base;
        let result = GitHubProvider::new(config, Arc::new(UreqGitHubTransport)).query_page(
            &TicketQuery::default(),
            Some(&next),
            100,
        );
        let first = server.join().unwrap();
        assert!(result.unwrap().next_cursor.is_none());
        assert_eq!(first, format!("GET {request_target} HTTP/1.1\r\n"));
    }
}

#[test]
fn hs2_tag2zp_device_cycles_and_existing_hundred_page_cap_are_errors() {
    for cycle in [true, false] {
        let first = "https://ghe.test/api/v3/user/installations?per_page=100";
        let responses = (0..100)
            .map(|i| {
                let next = if cycle {
                    first.to_owned()
                } else {
                    format!("https://ghe.test/api/v3/user/installations?page={}", i + 2)
                };
                page(json!({"installations": []}), Some(&next))
            })
            .collect();
        let transport = pages(responses);
        let error = GitHubDeviceClient::new("synthetic", "https://ghe.test", transport.clone())
            .repository_access("synthetic-token")
            .unwrap_err()
            .to_string();
        assert!(error.contains(if cycle { "cycle" } else { "limit" }));
        assert_eq!(
            transport.urls.lock_or_recover().len(),
            if cycle { 1 } else { 100 }
        );
    }
}

#[test]
fn hs2_tag2zp_provider_full_and_keyset_walks_reject_two_page_cycles() {
    for keyset in [false, true] {
        let second = "https://ghe.test/api/v3/repos/synthetic/issues/issues?page=2";
        let third = "https://ghe.test/api/v3/repos/synthetic/issues/issues?page=3";
        let transport = pages(vec![
            page(json!([]), Some(second)),
            page(json!([]), Some(third)),
            page(json!([]), Some(second)),
        ]);
        let provider = provider(transport.clone());
        let error = if keyset {
            provider
                .query_after(
                    &TicketQuery {
                        sort: SortKey::Created,
                        ..TicketQuery::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap_err()
        } else {
            provider.query(&TicketQuery::default()).unwrap_err()
        };
        assert!(error.to_string().contains("cycle"));
        assert_eq!(transport.urls.lock_or_recover().len(), 3);
    }
}

#[test]
fn hs2_tag2zp_query_payload_cannot_authorize_an_unrelated_page_path() {
    for next in [
        "https://ghe.test/api/v3/not-an-issue-page?next=/issues?",
        "https://ghe.test/api/v3/repos/another/repository/issues?page=2",
        "https://ghe.test/api/v3/repos/synthetic/issues/issues/7?next=/issues?",
    ] {
        let transport = pages(vec![]);
        assert!(
            provider(transport.clone())
                .query_page(&TicketQuery::default(), Some(next), 100)
                .is_err()
        );
        assert!(transport.urls.lock_or_recover().is_empty());
        let transport = pages(vec![page(json!([]), Some(next))]);
        assert!(
            provider(transport.clone())
                .query_page(&TicketQuery::default(), None, 100)
                .is_err()
        );
        assert_eq!(transport.urls.lock_or_recover().len(), 1);
    }
}

#[test]
fn hs2_tag2zp_provider_keyset_empty_obsolete_hint_restarts_without_a_false_cycle() {
    let resume = "https://ghe.test/api/v3/repos/synthetic/issues/issues?page=2";
    let transport = pages(vec![
        page(json!([]), None),
        page(json!([]), Some(resume)),
        page(json!([]), None),
    ]);
    let result = provider(transport.clone())
        .query_after(
            &TicketQuery {
                sort: SortKey::Created,
                ..TicketQuery::default()
            },
            None,
            Some(resume),
            100,
        )
        .unwrap();
    assert!(result.exhausted);
    assert!(result.items.is_empty());
    assert_eq!(transport.urls.lock_or_recover().len(), 3);
}

#[test]
fn hs2_tag2zp_provider_nonempty_keyset_resume_and_obsolete_restart_keep_boundary() {
    let resume = "https://ghe.test/api/v3/repos/synthetic/issues/issues?page=2";
    let third = "https://ghe.test/api/v3/repos/synthetic/issues/issues?page=3";
    let query = TicketQuery {
        sort: SortKey::Created,
        ..TicketQuery::default()
    };
    let issue = |number: u64, title: &str, body: &str| {
        json!({
            "number": number, "title": title, "body": body, "state": "open",
            "html_url": format!("https://ghe.test/synthetic/issues/issues/{number}"),
            "created_at": format!("2026-10-01T00:00:{number:02}Z"),
            "updated_at": format!("2026-10-01T00:00:{number:02}Z"),
            "labels": [], "assignees": []
        })
    };
    let first_page = || {
        page(
            json!([issue(1, "first", ""), issue(2, "boundary", "")]),
            Some(resume),
        )
    };
    let second_page = || {
        page(
            json!([issue(3, "third", ""), issue(4, "fourth", "")]),
            Some(third),
        )
    };
    let last_page = || page(json!([issue(5, "fifth", "")]), None);
    let uninterrupted = provider(pages(vec![first_page(), second_page(), last_page()]))
        .query_after(&query, None, None, 100)
        .unwrap();
    assert!(uninterrupted.exhausted);
    let after = MergeKey::from_ticket(&uninterrupted.items[1].ticket);
    let expected = uninterrupted.items[2..]
        .iter()
        .map(|item| item.ticket.native_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(expected, ["3", "4", "5"]);
    for obsolete in [false, true] {
        let responses = if obsolete {
            // The resume page has shifted past the boundary; restart at page one.
            vec![second_page(), first_page(), second_page(), last_page()]
        } else {
            // A valid hint includes the boundary row, which must not be re-emitted.
            vec![
                page(
                    json!([
                        issue(2, "boundary", ""),
                        issue(3, "third", ""),
                        issue(4, "fourth", "")
                    ]),
                    Some(third),
                ),
                last_page(),
            ]
        };
        let transport = pages(responses);
        let result = provider(transport.clone())
            .query_after(&query, Some(&after), Some(resume), 100)
            .unwrap();
        assert!(result.exhausted);
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.ticket.native_id.clone())
                .collect::<Vec<_>>(),
            expected
        );
        let requests = transport.urls.lock_or_recover();
        assert_eq!(requests.len(), if obsolete { 4 } else { 2 });
        assert_eq!(requests[0], resume);
        if obsolete {
            assert!(requests[1].contains("issues?state=all&direction=asc&per_page=100"));
            assert_eq!(requests[2], resume);
        }
    }
}

#[test]
fn hs2_tag2zp_provider_walks_fail_at_the_explicit_request_budget() {
    for keyset in [false, true] {
        let transport = pages(
            (0..ISSUE_PAGE_REQUEST_LIMIT)
                .map(|index| {
                    page(
                        json!([]),
                        Some(&format!(
                            "https://ghe.test/api/v3/repos/synthetic/issues/issues?page={}",
                            index + 2
                        )),
                    )
                })
                .collect(),
        );
        let provider = provider(transport.clone());
        let error = if keyset {
            provider
                .query_after(
                    &TicketQuery {
                        sort: SortKey::Created,
                        ..TicketQuery::default()
                    },
                    None,
                    None,
                    100,
                )
                .unwrap_err()
        } else {
            provider.query(&TicketQuery::default()).unwrap_err()
        };
        assert!(error.to_string().contains("request limit exceeded"));
        assert_eq!(
            transport.urls.lock_or_recover().len(),
            ISSUE_PAGE_REQUEST_LIMIT
        );
    }
}

#[test]
fn hs2_tag2zp_real_ureq_device_flow_never_contacts_foreign_loopback() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    let selected = TcpListener::bind("127.0.0.1:0").unwrap();
    selected.set_nonblocking(true).unwrap();
    let foreign = TcpListener::bind("127.0.0.1:0").unwrap();
    foreign.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", selected.local_addr().unwrap());
    let next = format!(
        "http://{}/api/v3/user/installations?page=2",
        foreign.local_addr().unwrap()
    );
    let server = std::thread::spawn(move || {
        // HS2-TAG2ZP: pre-connect client failures cannot strand fixture cleanup.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (mut stream, _) = loop {
            match selected.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "selected request missing"
                    );
                    std::thread::yield_now();
                }
                Err(error) => panic!("selected listener failed: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
        }
        let body = "{\"installations\":[]}";
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nLink: <{next}>; rel=\"next\"\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let result = GitHubDeviceClient::live("synthetic", origin).repository_access("synthetic-token");
    server.join().unwrap();
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("selected API origin")
    );
    assert_eq!(
        foreign.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
