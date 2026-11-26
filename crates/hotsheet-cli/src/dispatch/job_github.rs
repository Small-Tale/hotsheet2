//! The HTTP side of the GitHub Actions watch (HS2-3R9V0R): `hotsheet_jobs::GithubApi`
//! over the shared `UreqGitHubTransport`. The token comes from `GH_TOKEN` or
//! `GITHUB_TOKEN`, otherwise from the signed-in `gh` CLI. It lives in memory only: it is
//! never written to a Job, a log, or the process journal.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use hotsheet_extsync::github::{GitHubTransport, UreqGitHubTransport};
use hotsheet_jobs::{
    ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, Clock, GithubApi, SystemClock,
};

const DEFAULT_API: &str = "https://api.github.com";
/// Overrides the API root, for GitHub Enterprise Server (`https://<host>/api/v3`).
const API_URL_ENV: &str = "HOTSHEET_GITHUB_API_URL";
/// A provider's requested delay is honored up to this long.
const MAX_RETRY_AFTER_MS: u64 = 60 * 60 * 1000;
/// Without credentials there is nothing to retry quickly.
const NO_TOKEN_RETRY_MS: u64 = 60_000;
/// How long a failed `gh auth token` is remembered. Each attempt is a child launch in
/// the bounded process journal, so a machine without `gh` must not fill it.
const GH_RETRY_MS: u64 = 15 * 60_000;

pub(super) struct UreqGithubApi {
    home: PathBuf,
    base: Option<String>,
    token: Option<hotsheet_sync::secret::SecretString>,
    /// Do not launch `gh` again before this time.
    gh_not_before_ms: u64,
    transport: UreqGitHubTransport,
}

impl UreqGithubApi {
    pub(super) fn new(home: &Path) -> Self {
        let configured = std::env::var(API_URL_ENV).ok();
        Self {
            home: home.to_owned(),
            base: api_base(configured.as_deref()),
            token: None,
            gh_not_before_ms: 0,
            transport: UreqGitHubTransport,
        }
    }

    fn token(&mut self) -> Option<hotsheet_sync::secret::SecretString> {
        if self.token.is_none() {
            self.token = ["GH_TOKEN", "GITHUB_TOKEN"]
                .into_iter()
                .filter_map(|name| std::env::var(name).ok())
                .map(|value| value.trim().to_owned())
                .find(|value| !value.is_empty())
                .map(hotsheet_sync::secret::SecretString::from);
        }
        let now = SystemClock.now_ms();
        if self.token.is_none() && now >= self.gh_not_before_ms {
            self.token = self
                .gh_token()
                .map(hotsheet_sync::secret::SecretString::from);
            if self.token.is_none() {
                self.gh_not_before_ms = now.saturating_add(GH_RETRY_MS);
            }
        }
        self.token.clone()
    }

    /// Ask the signed-in `gh` CLI. One launch per token, recorded in the process journal
    /// by purpose only.
    fn gh_token(&self) -> Option<String> {
        let mut command = hotsheet_ticketing::git::launch("gh");
        command.args(["auth", "token"]).stderr(Stdio::null());
        let output = hotsheet_process_journal::LaunchContext {
            journal: hotsheet_process_journal::ProcessJournal::at(&self.home),
            supervisor: "hotsheet-cli",
            purpose: hotsheet_process_journal::Purpose::CliExternal,
            project_id: None,
            session_id: None,
            binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
        }
        .output(&mut command)
        .ok()?;
        let token = String::from_utf8(output.stdout).ok()?.trim().to_owned();
        (output.status.success() && !token.is_empty()).then_some(token)
    }
}

/// The API root to use, or `None` when the configured one must not receive a token:
/// only HTTPS is accepted, except plain HTTP to this machine for tests.
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

fn retry_after_ms(
    status: u16,
    headers: &std::collections::HashMap<String, String>,
    now_ms: u64,
) -> Option<u64> {
    let number = |name: &str| headers.get(name)?.trim().parse::<u64>().ok();
    let delay = number("retry-after")
        .map(|seconds| seconds.saturating_mul(1000))
        .or_else(|| {
            // A spent primary rate limit names the moment it resets.
            let spent = matches!(status, 403 | 429) && number("x-ratelimit-remaining") == Some(0);
            let reset_ms = number("x-ratelimit-reset")?.saturating_mul(1000);
            spent.then(|| reset_ms.saturating_sub(now_ms).max(1000))
        })?;
    Some(delay.min(MAX_RETRY_AFTER_MS))
}

impl GithubApi for UreqGithubApi {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let unavailable = ApiUnavailable {
            retry_after_ms: Some(NO_TOKEN_RETRY_MS),
        };
        let base = self.base.clone().ok_or(unavailable)?;
        let token = self.token().ok_or(unavailable)?;
        let mut headers = vec![
            ("Authorization", format!("Bearer {}", token.expose())),
            ("Accept", "application/vnd.github+json".to_owned()),
            ("X-GitHub-Api-Version", "2022-11-28".to_owned()),
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
            // The token was revoked or expired; resolve it again on the next request.
            self.token = None;
        }
        Ok(ApiResponse {
            status: response.status,
            etag: response.headers.get("etag").cloned(),
            retry_after_ms: retry_after_ms(
                response.status,
                &response.headers,
                SystemClock.now_ms(),
            ),
            body: response.body,
        })
    }
}

/// `owner/name` from a Git remote URL; HS2-X6FB9V moved it to the jobs library so the
/// shared runtime can start a watch too.
pub(super) use hotsheet_jobs::repo_from_remote;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn a_token_is_only_ever_sent_over_https_or_to_this_machine() {
        assert_eq!(api_base(None).as_deref(), Some("https://api.github.com"));
        assert_eq!(
            api_base(Some("  ")).as_deref(),
            Some("https://api.github.com")
        );
        assert_eq!(
            api_base(Some("https://ghe.example.test/api/v3/")).as_deref(),
            Some("https://ghe.example.test/api/v3")
        );
        for local in [
            "http://127.0.0.1:8080",
            "http://localhost:9/x",
            "http://[::1]:7",
        ] {
            assert_eq!(api_base(Some(local)).as_deref(), Some(local));
        }
        for refused in [
            "http://api.github.com",
            "http://127.0.0.1.example.test",
            "http://localhost.example.test:80",
            "ftp://127.0.0.1",
            "api.github.com",
        ] {
            assert_eq!(api_base(Some(refused)), None, "{refused}");
        }
    }

    #[test]
    fn retry_delays_come_from_the_providers_headers() {
        let headers = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect()
        };
        assert_eq!(retry_after_ms(200, &headers(&[]), 0), None);
        assert_eq!(
            retry_after_ms(429, &headers(&[("retry-after", "30")]), 0),
            Some(30_000)
        );
        assert_eq!(
            retry_after_ms(403, &headers(&[("retry-after", "999999")]), 0),
            Some(MAX_RETRY_AFTER_MS)
        );
        let spent = headers(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "100")]);
        assert_eq!(retry_after_ms(403, &spent, 40_000), Some(60_000));
        assert_eq!(retry_after_ms(403, &spent, 200_000), Some(1000));
        // A reset time alone is not a reason to wait, and neither is a permission error.
        assert_eq!(retry_after_ms(200, &spent, 40_000), None);
        let left = headers(&[
            ("x-ratelimit-remaining", "12"),
            ("x-ratelimit-reset", "100"),
        ]);
        assert_eq!(retry_after_ms(403, &left, 40_000), None);
    }

    #[test]
    fn a_repository_is_read_from_every_usual_remote_spelling() {
        for url in [
            "git@github.com:acme/widgets.git",
            "https://github.com/acme/widgets",
            "https://github.com/acme/widgets.git\n",
            "ssh://git@github.com/acme/widgets.git",
            "https://ghe.example.test/acme/widgets/",
        ] {
            assert_eq!(
                repo_from_remote(url).as_deref(),
                Some("acme/widgets"),
                "{url}"
            );
        }
        for url in ["", "widgets", "https://github.com/widgets", "/local/path"] {
            assert_eq!(repo_from_remote(url), None, "{url}");
        }
    }
}
