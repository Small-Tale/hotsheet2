//! HS2-TPAHYA: the HTTP side of the GitLab CI watch, the shared REST port
//! (`hotsheet_jobs::GithubApi`) over the same HTTP transport as GitHub. The token comes
//! from `GITLAB_TOKEN` and is sent as `PRIVATE-TOKEN`; it lives in memory only and is
//! never written to a Job, a log, or the process journal. The API root is
//! `HOTSHEET_GITLAB_API_URL` (default `https://gitlab.com/api/v4`), HTTPS only except
//! loopback HTTP for tests.

use hotsheet_extsync::github::{GitHubTransport, UreqGitHubTransport};
use hotsheet_jobs::{ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, GithubApi};

const DEFAULT_API: &str = "https://gitlab.com/api/v4";
/// Overrides the API root, for a self-managed GitLab (`https://<host>/api/v4`).
const API_URL_ENV: &str = "HOTSHEET_GITLAB_API_URL";
/// The variable the token is read from.
const TOKEN_ENV: &str = "GITLAB_TOKEN";
/// A provider's requested delay is honored up to this long.
const MAX_RETRY_AFTER_MS: u64 = 60 * 60 * 1000;
/// Without credentials there is nothing to retry quickly.
const NO_TOKEN_RETRY_MS: u64 = 60_000;

pub(super) struct UreqGitlabApi {
    base: Option<String>,
    transport: UreqGitHubTransport,
}

impl UreqGitlabApi {
    pub(super) fn new() -> Self {
        let configured = std::env::var(API_URL_ENV).ok();
        Self {
            base: api_base(configured.as_deref()),
            transport: UreqGitHubTransport,
        }
    }
}

/// The API root to use, or `None` when the configured one must not receive a token.
fn api_base(configured: Option<&str>) -> Option<String> {
    let base = configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_API)
        .trim_end_matches('/');
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .into_iter()
        .any(|prefix| {
            base.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
        });
    (base.starts_with("https://") || loopback).then(|| base.to_owned())
}

/// `Retry-After`, or the moment a spent rate limit resets (`RateLimit-Reset`).
fn retry_after_ms(
    status: u16,
    headers: &std::collections::HashMap<String, String>,
    now_ms: u64,
) -> Option<u64> {
    let number = |name: &str| headers.get(name)?.trim().parse::<u64>().ok();
    let delay = number("retry-after")
        .map(|seconds| seconds.saturating_mul(1000))
        .or_else(|| {
            let spent = status == 429 && number("ratelimit-remaining") == Some(0);
            let reset_ms = number("ratelimit-reset")?.saturating_mul(1000);
            spent.then(|| reset_ms.saturating_sub(now_ms).max(1000))
        })?;
    Some(delay.min(MAX_RETRY_AFTER_MS))
}

impl GithubApi for UreqGitlabApi {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let unavailable = ApiUnavailable {
            retry_after_ms: Some(NO_TOKEN_RETRY_MS),
        };
        let base = self.base.clone().ok_or(unavailable)?;
        let token = std::env::var(TOKEN_ENV)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or(unavailable)?;
        let mut headers = vec![
            ("PRIVATE-TOKEN", token),
            ("Accept", "application/json".to_owned()),
            ("User-Agent", "hotsheet-cli".to_owned()),
        ];
        if let Some(etag) = &request.if_none_match {
            headers.push(("If-None-Match", etag.clone()));
        }
        let method = match request.method {
            ApiMethod::Get => "GET",
            ApiMethod::Post => "POST",
        };
        let url = format!("{base}/{}", request.path);
        let response = self
            .transport
            .request(method, &url, &headers, None)
            .map_err(|_| ApiUnavailable {
                retry_after_ms: None,
            })?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        Ok(ApiResponse {
            status: response.status,
            etag: response.headers.get("etag").cloned(),
            retry_after_ms: retry_after_ms(response.status, &response.headers, now_ms),
            body: response.body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn hs2_tpahya_a_token_is_only_ever_sent_over_https_or_to_this_machine() {
        assert_eq!(api_base(None).as_deref(), Some(DEFAULT_API));
        assert_eq!(
            api_base(Some("https://gitlab.example.test/api/v4/")).as_deref(),
            Some("https://gitlab.example.test/api/v4")
        );
        assert_eq!(
            api_base(Some("http://127.0.0.1:9/api/v4")).as_deref(),
            Some("http://127.0.0.1:9/api/v4")
        );
        for refused in [
            "http://gitlab.example.test",
            "http://localhost.example.test",
        ] {
            assert_eq!(api_base(Some(refused)), None, "{refused}");
        }
    }

    #[test]
    fn hs2_tpahya_rate_limits_set_the_retry() {
        let headers = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect()
        };
        assert_eq!(
            retry_after_ms(429, &headers(&[("retry-after", "7")]), 0),
            Some(7_000)
        );
        assert_eq!(
            retry_after_ms(
                429,
                &headers(&[("ratelimit-remaining", "0"), ("ratelimit-reset", "100")]),
                40_000
            ),
            Some(60_000)
        );
        assert_eq!(retry_after_ms(200, &headers(&[]), 0), None);
    }
}
