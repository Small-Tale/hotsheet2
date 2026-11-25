//! HS2-TPAHYA: the HTTP side of the GitLab CI watch, the shared REST port
//! (`hotsheet_jobs::GithubApi`) over the same HTTP transport as GitHub. The token comes
//! from `GITLAB_TOKEN`, otherwise (HS2-RTEQ7R) from the signed-in `glab` CLI for the API
//! root's host, and is sent as `PRIVATE-TOKEN`; it lives in memory only and is never
//! written to a Job, a log, or the process journal. The API root is
//! `HOTSHEET_GITLAB_API_URL` (default `https://gitlab.com/api/v4`), HTTPS only except
//! loopback HTTP for tests.

use std::path::{Path, PathBuf};
use std::process::Stdio;

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
/// HS2-RTEQ7R: how long a failed `glab` lookup is remembered. Each attempt is a child
/// launch in the bounded process journal, so a machine without `glab` must not fill it.
const GLAB_RETRY_MS: u64 = 15 * 60_000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// HS2-RTEQ7R: where a GitLab token comes from, for `job watch status`. Never the token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TokenSource {
    Environment,
    Glab,
}

impl TokenSource {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Environment => "GITLAB_TOKEN",
            Self::Glab => "glab",
        }
    }
}

/// The token from `GITLAB_TOKEN`, when set.
fn environment_token() -> Option<String> {
    std::env::var(TOKEN_ENV)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The host part of an API root, which `glab` keys its sign-ins by.
fn api_host(base: &str) -> Option<String> {
    let rest = base.split_once("://")?.1;
    let host = rest.split('/').next()?;
    (!host.is_empty()).then(|| host.to_owned())
}

/// HS2-RTEQ7R: ask the signed-in `glab` CLI for its token for `host`. One launch,
/// recorded in the process journal by purpose only; the token never leaves memory.
fn glab_token(home: &Path, host: &str) -> Option<String> {
    let mut command = hotsheet_ticketing::git::launch("glab");
    command
        .args(["config", "get", "token", "--host", host])
        .stderr(Stdio::null());
    let output = hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(home),
        supervisor: "hotsheet-cli",
        purpose: hotsheet_process_journal::Purpose::CliExternal,
        project_id: None,
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
    .output(&mut command)
    .ok()?;
    let token = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !token.is_empty() && !token.contains(char::is_whitespace))
        .then_some(token)
}

/// HS2-RTEQ7R: where this machine would take a GitLab token from now, for
/// `job watch status`. Launches `glab` only when the variable is unset.
pub(super) fn token_source(home: &Path) -> Option<TokenSource> {
    if environment_token().is_some() {
        return Some(TokenSource::Environment);
    }
    let configured = std::env::var(API_URL_ENV).ok();
    let host = api_base(configured.as_deref()).and_then(|base| api_host(&base))?;
    glab_token(home, &host).map(|_| TokenSource::Glab)
}

pub(super) struct UreqGitlabApi {
    home: PathBuf,
    base: Option<String>,
    /// A `glab` token, once read; the variable is read fresh on every request.
    glab: Option<String>,
    /// Do not launch `glab` again before this time.
    glab_not_before_ms: u64,
    transport: UreqGitHubTransport,
}

impl UreqGitlabApi {
    pub(super) fn new(home: &Path) -> Self {
        let configured = std::env::var(API_URL_ENV).ok();
        Self {
            home: home.to_owned(),
            base: api_base(configured.as_deref()),
            glab: None,
            glab_not_before_ms: 0,
            transport: UreqGitHubTransport,
        }
    }

    fn token(&mut self, base: &str) -> Option<String> {
        if let Some(token) = environment_token() {
            return Some(token);
        }
        let now = now_ms();
        if self.glab.is_none() && now >= self.glab_not_before_ms {
            self.glab = api_host(base).and_then(|host| glab_token(&self.home, &host));
            if self.glab.is_none() {
                self.glab_not_before_ms = now.saturating_add(GLAB_RETRY_MS);
            }
        }
        self.glab.clone()
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
        let token = self.token(&base).ok_or(unavailable)?;
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
            ApiMethod::Put => "PUT",
            ApiMethod::Patch => "PATCH",
        };
        let url = format!("{base}/{}", request.path);
        let response = self
            .transport
            .request(method, &url, &headers, request.body.as_ref())
            .map_err(|_| ApiUnavailable {
                retry_after_ms: None,
            })?;
        if response.status == 401 {
            // A revoked or expired `glab` token is read again on the next request.
            self.glab = None;
        }
        let now_ms = now_ms();
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
    fn hs2_rteq7r_glab_is_asked_for_the_api_roots_host() {
        assert_eq!(api_host(DEFAULT_API).as_deref(), Some("gitlab.com"));
        assert_eq!(
            api_host("https://gitlab.example.test:8443/api/v4").as_deref(),
            Some("gitlab.example.test:8443")
        );
        assert_eq!(api_host("not a url"), None);
        assert_eq!(TokenSource::Glab.code(), "glab");
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
