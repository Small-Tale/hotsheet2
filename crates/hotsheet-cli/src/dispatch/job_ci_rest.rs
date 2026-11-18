//! HS2-17HVNP: the HTTP side of the Buildkite and CircleCI watches, the shared REST
//! port (`hotsheet_jobs::GithubApi`) over the same HTTP transport as GitHub and GitLab.
//! Each provider's token comes from its environment variable and is sent in its own
//! header; it lives in memory only and is never written to a Job, a log, or the process
//! journal. The API root is configurable for tests and self-hosted endpoints, HTTPS only
//! except loopback HTTP.
//!
//! - Buildkite REST v2 (<https://buildkite.com/docs/apis/rest-api>): root
//!   `https://api.buildkite.com/v2` (`HOTSHEET_BUILDKITE_API_URL`), token
//!   `BUILDKITE_API_TOKEN` as `Authorization: Bearer`. A spent limit answers 429 with
//!   `RateLimit-Reset`, the seconds until it resets
//!   (<https://buildkite.com/docs/apis/rest-api/limits>).
//! - CircleCI API v2 (<https://circleci.com/docs/api/v2/index.html>): root
//!   `https://circleci.com/api/v2` (`HOTSHEET_CIRCLECI_API_URL`), token `CIRCLECI_TOKEN`
//!   as `Circle-Token`. A spent limit answers 429 with `Retry-After`.

use hotsheet_extsync::github::{GitHubTransport, UreqGitHubTransport};
use hotsheet_jobs::{ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, GithubApi};

/// A provider's requested delay is honored up to this long.
const MAX_RETRY_AFTER_MS: u64 = 60 * 60 * 1000;
/// Without credentials there is nothing to retry quickly.
const NO_TOKEN_RETRY_MS: u64 = 60_000;

/// One provider's REST settings.
#[derive(Clone, Copy, Debug)]
pub(super) struct RestProvider {
    default_api: &'static str,
    url_env: &'static str,
    token_env: &'static str,
    /// The header the token is sent in, and whether it is a `Bearer` value.
    header: &'static str,
    bearer: bool,
}

pub(super) const BUILDKITE: RestProvider = RestProvider {
    default_api: "https://api.buildkite.com/v2",
    url_env: "HOTSHEET_BUILDKITE_API_URL",
    token_env: "BUILDKITE_API_TOKEN",
    header: "Authorization",
    bearer: true,
};

pub(super) const CIRCLECI: RestProvider = RestProvider {
    default_api: "https://circleci.com/api/v2",
    url_env: "HOTSHEET_CIRCLECI_API_URL",
    token_env: "CIRCLECI_TOKEN",
    header: "Circle-Token",
    bearer: false,
};

impl RestProvider {
    /// The token from the provider's variable, when set. Read fresh on every request.
    fn token(self) -> Option<String> {
        std::env::var(self.token_env)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    /// For `job watch status`: whether a token is configured, never the token.
    pub(super) fn has_token(self) -> bool {
        self.token().is_some()
    }

    pub(super) fn token_env(self) -> &'static str {
        self.token_env
    }
}

/// The API root to use, or `None` when the configured one must not receive a token.
fn api_base(provider: RestProvider, configured: Option<&str>) -> Option<String> {
    let base = configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(provider.default_api)
        .trim_end_matches('/');
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .into_iter()
        .any(|prefix| {
            base.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
        });
    (base.starts_with("https://") || loopback).then(|| base.to_owned())
}

/// `Retry-After` in seconds, or for a spent limit `RateLimit-Reset` in seconds.
fn retry_after_ms(status: u16, headers: &std::collections::HashMap<String, String>) -> Option<u64> {
    let number = |name: &str| headers.get(name)?.trim().parse::<u64>().ok();
    let seconds = number("retry-after").or_else(|| {
        (status == 429)
            .then(|| number("ratelimit-reset"))
            .flatten()
            .map(|seconds| seconds.max(1))
    })?;
    Some(seconds.saturating_mul(1000).min(MAX_RETRY_AFTER_MS))
}

pub(super) struct UreqCiRestApi {
    provider: RestProvider,
    base: Option<String>,
    transport: UreqGitHubTransport,
}

impl UreqCiRestApi {
    pub(super) fn new(provider: RestProvider) -> Self {
        let configured = std::env::var(provider.url_env).ok();
        Self {
            provider,
            base: api_base(provider, configured.as_deref()),
            transport: UreqGitHubTransport,
        }
    }
}

impl GithubApi for UreqCiRestApi {
    fn send(&mut self, request: &ApiRequest) -> Result<ApiResponse, ApiUnavailable> {
        let unavailable = ApiUnavailable {
            retry_after_ms: Some(NO_TOKEN_RETRY_MS),
        };
        let base = self.base.clone().ok_or(unavailable)?;
        let token = self.provider.token().ok_or(unavailable)?;
        let value = if self.provider.bearer {
            format!("Bearer {token}")
        } else {
            token
        };
        let mut headers = vec![
            (self.provider.header, value),
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
        };
        let url = format!("{base}/{}", request.path);
        let response = self
            .transport
            .request(method, &url, &headers, None)
            .map_err(|_| ApiUnavailable {
                retry_after_ms: None,
            })?;
        Ok(ApiResponse {
            status: response.status,
            etag: response.headers.get("etag").cloned(),
            retry_after_ms: retry_after_ms(response.status, &response.headers),
            body: response.body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn hs2_17hvnp_a_token_is_only_ever_sent_over_https_or_to_this_machine() {
        for provider in [BUILDKITE, CIRCLECI] {
            assert_eq!(
                api_base(provider, None).as_deref(),
                Some(provider.default_api)
            );
            assert_eq!(
                api_base(provider, Some("http://127.0.0.1:9/v2/")).as_deref(),
                Some("http://127.0.0.1:9/v2")
            );
            assert_eq!(
                api_base(provider, Some("https://ci.example.test/api/v2")).as_deref(),
                Some("https://ci.example.test/api/v2")
            );
            for refused in ["http://ci.example.test", "http://localhost.example.test"] {
                assert_eq!(api_base(provider, Some(refused)), None, "{refused}");
            }
        }
        assert_eq!(BUILDKITE.token_env(), "BUILDKITE_API_TOKEN");
        assert_eq!(CIRCLECI.token_env(), "CIRCLECI_TOKEN");
    }

    #[test]
    fn hs2_17hvnp_rate_limits_set_the_retry() {
        let headers = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect()
        };
        // CircleCI: Retry-After seconds.
        assert_eq!(
            retry_after_ms(429, &headers(&[("retry-after", "7")])),
            Some(7_000)
        );
        // Buildkite: seconds until the spent limit resets.
        assert_eq!(
            retry_after_ms(
                429,
                &headers(&[("ratelimit-remaining", "0"), ("ratelimit-reset", "42")])
            ),
            Some(42_000)
        );
        assert_eq!(
            retry_after_ms(200, &headers(&[("ratelimit-reset", "42")])),
            None
        );
        assert_eq!(
            retry_after_ms(429, &headers(&[("retry-after", "999999")])),
            Some(MAX_RETRY_AFTER_MS)
        );
    }
}
