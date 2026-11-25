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
//! - HS2-7H0S8R: Jenkins' JSON API
//!   (<https://www.jenkins.io/doc/book/using/remote-access-api/>): every Jenkins is
//!   self-hosted, so there is no default root; `HOTSHEET_JENKINS_URL` names it. The user
//!   `JENKINS_USER` and API token `JENKINS_API_TOKEN` are sent as HTTP Basic, which
//!   Jenkins accepts without a CSRF crumb.
//! - HS2-JZ1DD2: Azure DevOps Build REST 7.1
//!   (<https://learn.microsoft.com/en-us/rest/api/azure/devops/build/?view=azure-devops-rest-7.1>):
//!   root `https://dev.azure.com` (`HOTSHEET_AZURE_DEVOPS_API_URL`), personal access token
//!   `AZURE_DEVOPS_EXT_PAT` (the Azure CLI's variable) as HTTP Basic with an empty user
//!   (<https://learn.microsoft.com/en-us/azure/devops/organizations/accounts/use-personal-access-tokens-to-authenticate>).
//!   A spent limit answers 429 with `Retry-After`.

use base64::Engine;
use hotsheet_extsync::github::{GitHubTransport, UreqGitHubTransport};
use hotsheet_jobs::{ApiMethod, ApiRequest, ApiResponse, ApiUnavailable, GithubApi};

/// A provider's requested delay is honored up to this long.
const MAX_RETRY_AFTER_MS: u64 = 60 * 60 * 1000;
/// Without credentials there is nothing to retry quickly.
const NO_TOKEN_RETRY_MS: u64 = 60_000;

/// How a credential is sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scheme {
    /// The token as the header's whole value.
    Raw,
    /// `Bearer <token>`.
    Bearer,
    /// HS2-7H0S8R: `Basic base64(<user>:<token>)`; the user from `user_env`, or empty.
    Basic { user_env: Option<&'static str> },
}

/// One way to authenticate: a token variable, the header it goes in and its scheme.
#[derive(Clone, Copy, Debug)]
struct Credential {
    token_env: &'static str,
    header: &'static str,
    scheme: Scheme,
}

/// One provider's REST settings.
#[derive(Clone, Copy, Debug)]
pub(super) struct RestProvider {
    /// `None` when the provider is only ever self-hosted (HS2-7H0S8R).
    default_api: Option<&'static str>,
    url_env: &'static str,
    /// Tried in order; the first one fully configured is used.
    credentials: &'static [Credential],
}

pub(super) const BUILDKITE: RestProvider = RestProvider {
    default_api: Some("https://api.buildkite.com/v2"),
    url_env: "HOTSHEET_BUILDKITE_API_URL",
    credentials: &[Credential {
        token_env: "BUILDKITE_API_TOKEN",
        header: "Authorization",
        scheme: Scheme::Bearer,
    }],
};

pub(super) const CIRCLECI: RestProvider = RestProvider {
    default_api: Some("https://circleci.com/api/v2"),
    url_env: "HOTSHEET_CIRCLECI_API_URL",
    credentials: &[Credential {
        token_env: "CIRCLECI_TOKEN",
        header: "Circle-Token",
        scheme: Scheme::Raw,
    }],
};

/// HS2-7H0S8R: a Jenkins controller, named by `HOTSHEET_JENKINS_URL`.
pub(super) const JENKINS: RestProvider = RestProvider {
    default_api: None,
    url_env: "HOTSHEET_JENKINS_URL",
    credentials: &[Credential {
        token_env: "JENKINS_API_TOKEN",
        header: "Authorization",
        scheme: Scheme::Basic {
            user_env: Some("JENKINS_USER"),
        },
    }],
};

/// HS2-JZ1DD2: Azure DevOps Services, or a server named by
/// `HOTSHEET_AZURE_DEVOPS_API_URL`.
pub(super) const AZURE_PIPELINES: RestProvider = RestProvider {
    default_api: Some("https://dev.azure.com"),
    url_env: "HOTSHEET_AZURE_DEVOPS_API_URL",
    credentials: &[Credential {
        token_env: "AZURE_DEVOPS_EXT_PAT",
        header: "Authorization",
        scheme: Scheme::Basic { user_env: None },
    }],
};

/// A trimmed, non-empty environment value.
fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

impl RestProvider {
    /// The first fully configured credential as a header, read fresh on every request
    /// through `lookup`, with the variable that holds its token.
    fn authorization(
        self,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Option<(&'static str, String, &'static str)> {
        self.credentials.iter().find_map(|credential| {
            let token = lookup(credential.token_env)?;
            let value = match credential.scheme {
                Scheme::Raw => token,
                Scheme::Bearer => format!("Bearer {token}"),
                Scheme::Basic { user_env } => {
                    let user = match user_env {
                        Some(name) => lookup(name)?,
                        None => String::new(),
                    };
                    let pair = format!("{user}:{token}");
                    format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD.encode(pair)
                    )
                }
            };
            Some((credential.header, value, credential.token_env))
        })
    }

    /// For `job watch status`: the variable of the configured token, never the token.
    pub(super) fn token_source(self) -> Option<&'static str> {
        self.authorization(env_value).map(|(_, _, source)| source)
    }
}

/// The API root to use, or `None` when the configured one must not receive a token.
fn api_base(provider: RestProvider, configured: Option<&str>) -> Option<String> {
    let base = configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(provider.default_api)?
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
        let (header, value, _) = self.provider.authorization(env_value).ok_or(unavailable)?;
        let mut headers = vec![
            (header, value),
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
        for provider in [BUILDKITE, CIRCLECI, JENKINS, AZURE_PIPELINES] {
            assert_eq!(api_base(provider, None).as_deref(), provider.default_api);
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
        let all = |name: &str| Some(format!("value-of-{name}"));
        assert_eq!(
            BUILDKITE.authorization(all),
            Some((
                "Authorization",
                "Bearer value-of-BUILDKITE_API_TOKEN".to_owned(),
                "BUILDKITE_API_TOKEN"
            ))
        );
        assert_eq!(
            CIRCLECI.authorization(all),
            Some((
                "Circle-Token",
                "value-of-CIRCLECI_TOKEN".to_owned(),
                "CIRCLECI_TOKEN"
            ))
        );
    }

    #[test]
    fn hs2_jz1dd2_an_azure_pat_goes_as_basic_with_an_empty_user() {
        let pat = |name: &str| (name == "AZURE_DEVOPS_EXT_PAT").then(|| "p4t".to_owned());
        assert_eq!(
            AZURE_PIPELINES.authorization(pat),
            Some((
                "Authorization",
                // base64(":p4t")
                "Basic OnA0dA==".to_owned(),
                "AZURE_DEVOPS_EXT_PAT"
            ))
        );
        assert_eq!(AZURE_PIPELINES.authorization(|_| None), None);
        assert_eq!(
            api_base(AZURE_PIPELINES, None).as_deref(),
            Some("https://dev.azure.com")
        );
    }

    #[test]
    fn hs2_7h0s8r_jenkins_needs_its_url_and_sends_user_and_token_as_basic() {
        // Every Jenkins is self-hosted: without a configured root nothing is sent.
        assert_eq!(api_base(JENKINS, None), None);
        assert_eq!(api_base(JENKINS, Some("  ")), None);
        let lookup = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        let both = lookup(&[("JENKINS_USER", "ci-bot"), ("JENKINS_API_TOKEN", "t0ken")]);
        assert_eq!(
            JENKINS.authorization(both),
            Some((
                "Authorization",
                // base64("ci-bot:t0ken")
                "Basic Y2ktYm90OnQwa2Vu".to_owned(),
                "JENKINS_API_TOKEN"
            ))
        );
        // A token without its user, or a user without a token, is not a credential.
        assert_eq!(
            JENKINS.authorization(lookup(&[("JENKINS_API_TOKEN", "t0ken")])),
            None
        );
        assert_eq!(
            JENKINS.authorization(lookup(&[("JENKINS_USER", "ci-bot")])),
            None
        );
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
