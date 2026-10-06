//! GitHub App device authorization protocol. This module owns OAuth wire details but never
//! persists tokens; the server stores returned bundles in the OS credential store.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{GitHubTransport, UreqGitHubTransport};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitHubTokenBundle {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token_expires_in: Option<u64>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePoll {
    Pending,
    SlowDown,
    Authorized(GitHubTokenBundle),
    Expired,
    Denied,
}

#[derive(Debug, thiserror::Error)]
pub enum GitHubDeviceError {
    #[error("GitHub App sign-in is not configured (missing client id)")]
    MissingClientId,
    #[error("GitHub authorization request failed: {0}")]
    Transport(String),
    #[error("GitHub authorization returned HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("GitHub authorization response was invalid: {0}")]
    Invalid(String),
    #[error("GitHub authorization failed: {0}")]
    Rejected(String),
}

/// What the signed-in user can reach through the Hot Sheet GitHub App (HS2-27T5WT).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RepositoryAccess {
    /// `owner/name`, case-insensitively sorted, across every installation.
    pub repositories: Vec<String>,
    pub installations: Vec<AppInstallation>,
    /// Where to install the app on another account or organization, when known.
    pub install_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInstallation {
    pub account: String,
    /// `all` or `selected`: whether the installation grants every repository or a chosen few.
    pub selection: String,
    /// The installation's GitHub settings page, where its repository access is changed.
    pub settings_url: Option<String>,
}

pub struct GitHubDeviceClient {
    client_id: String,
    web_base: String,
    transport: Arc<dyn GitHubTransport>,
}

impl GitHubDeviceClient {
    pub fn live(client_id: impl Into<String>, web_base: impl Into<String>) -> Self {
        Self::new(client_id, web_base, Arc::new(UreqGitHubTransport))
    }

    pub fn new(
        client_id: impl Into<String>,
        web_base: impl Into<String>,
        transport: Arc<dyn GitHubTransport>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            web_base: web_base.into().trim_end_matches('/').to_owned(),
            transport,
        }
    }

    pub fn start(&self) -> Result<DeviceAuthorization, GitHubDeviceError> {
        self.require_client_id()?;
        let value = self.post("/login/device/code", json!({"client_id":self.client_id}))?;
        serde_json::from_value(value).map_err(|error| GitHubDeviceError::Invalid(error.to_string()))
    }

    pub fn poll(&self, device_code: &str) -> Result<DevicePoll, GitHubDeviceError> {
        self.require_client_id()?;
        let value = self.post(
            "/login/oauth/access_token",
            json!({"client_id":self.client_id,"device_code":device_code,"grant_type":"urn:ietf:params:oauth:grant-type:device_code"}),
        )?;
        match value.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => Ok(DevicePoll::Pending),
            Some("slow_down") => Ok(DevicePoll::SlowDown),
            Some("expired_token") => Ok(DevicePoll::Expired),
            Some("access_denied") => Ok(DevicePoll::Denied),
            Some(error) => Err(GitHubDeviceError::Rejected(
                value
                    .get("error_description")
                    .and_then(Value::as_str)
                    .unwrap_or(error)
                    .to_owned(),
            )),
            None => serde_json::from_value(value)
                .map(DevicePoll::Authorized)
                .map_err(|error| GitHubDeviceError::Invalid(error.to_string())),
        }
    }

    pub fn refresh(&self, refresh_token: &str) -> Result<GitHubTokenBundle, GitHubDeviceError> {
        self.require_client_id()?;
        serde_json::from_value(self.post(
            "/login/oauth/access_token",
            json!({"client_id":self.client_id,"refresh_token":refresh_token,"grant_type":"refresh_token"}),
        )?)
        .map_err(|error| GitHubDeviceError::Invalid(error.to_string()))
    }

    /// Resolve the signed-in user's login for account labels. Failure must not block sign-in.
    pub fn current_login(&self, access_token: &str) -> Result<String, GitHubDeviceError> {
        let headers = [
            ("Accept", "application/vnd.github+json".into()),
            ("Authorization", format!("Bearer {access_token}")),
            ("X-GitHub-Api-Version", "2022-11-28".into()),
            ("User-Agent", "hotsheet2".into()),
        ];
        let (user, _) = self.get_json_page(&format!("{}/user", self.api_base()), &headers)?;
        user.get("login")
            .and_then(Value::as_str)
            .filter(|login| !login.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| GitHubDeviceError::Invalid("user login missing".into()))
    }

    /// Lists repositories the signed-in user can access through this GitHub App installation.
    /// The access token remains on the server; callers receive repository names only.
    pub fn installed_repositories(
        &self,
        access_token: &str,
    ) -> Result<Vec<String>, GitHubDeviceError> {
        Ok(self.repository_access(access_token)?.repositories)
    }

    /// Every repository the signed-in user can reach through the app's installations, following
    /// `Link: rel="next"` pagination, plus what each installation grants and where to change it.
    /// GitHub only exposes repositories the app is installed on, so the installation details let
    /// clients explain a missing repository and link to fix it (HS2-27T5WT).
    pub fn repository_access(
        &self,
        access_token: &str,
    ) -> Result<RepositoryAccess, GitHubDeviceError> {
        let api_base = self.api_base();
        let headers = [
            ("Accept", "application/vnd.github+json".into()),
            ("Authorization", format!("Bearer {access_token}")),
            ("X-GitHub-Api-Version", "2022-11-28".into()),
            ("User-Agent", "hotsheet2".into()),
        ];
        let mut access = RepositoryAccess::default();
        let mut app_slug = None;
        for installation in self.get_pages(
            &format!("{api_base}/user/installations?per_page=100"),
            &headers,
            "installations",
        )? {
            let Some(id) = installation.get("id").and_then(Value::as_u64) else {
                continue;
            };
            let text = |key: &str| {
                installation
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            app_slug = app_slug.or_else(|| text("app_slug"));
            access.installations.push(AppInstallation {
                account: installation
                    .pointer("/account/login")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                selection: text("repository_selection").unwrap_or_else(|| "selected".into()),
                settings_url: text("html_url"),
            });
            access.repositories.extend(
                self.get_pages(
                    &format!("{api_base}/user/installations/{id}/repositories?per_page=100"),
                    &headers,
                    "repositories",
                )?
                .iter()
                .filter_map(|repository| repository.get("full_name").and_then(Value::as_str))
                .map(str::to_owned),
            );
        }
        access.repositories.sort_by_key(|name| name.to_lowercase());
        access.repositories.dedup();
        access.install_url =
            app_slug.map(|slug| format!("{}/apps/{slug}/installations/new", self.web_base));
        Ok(access)
    }

    /// Collect `key`'s array across every page of a paginated GitHub list.
    fn get_pages(
        &self,
        url: &str,
        headers: &[(&str, String)],
        key: &str,
    ) -> Result<Vec<Value>, GitHubDeviceError> {
        let mut items = Vec::new();
        let mut next = Some(url.to_owned());
        // A bound keeps a misbehaving `Link` header from looping forever.
        for _ in 0..100 {
            let Some(url) = next.take() else { break };
            let (value, link) = self.get_json_page(&url, headers)?;
            items.extend(
                value
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            next = link;
        }
        Ok(items)
    }

    fn require_client_id(&self) -> Result<(), GitHubDeviceError> {
        if self.client_id.trim().is_empty() {
            Err(GitHubDeviceError::MissingClientId)
        } else {
            Ok(())
        }
    }

    fn api_base(&self) -> String {
        if self.web_base == "https://github.com" {
            "https://api.github.com".to_owned()
        } else {
            format!("{}/api/v3", self.web_base)
        }
    }

    fn post(&self, path: &str, body: Value) -> Result<Value, GitHubDeviceError> {
        let response = self
            .transport
            .request(
                "POST",
                &format!("{}{path}", self.web_base),
                &[("Accept", "application/json".into())],
                Some(&body),
            )
            .map_err(GitHubDeviceError::Transport)?;
        let value: Value = serde_json::from_str(&response.body)
            .map_err(|error| GitHubDeviceError::Invalid(error.to_string()))?;
        if !(200..300).contains(&response.status) {
            return Err(GitHubDeviceError::Http {
                status: response.status,
                message: value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or(&response.body)
                    .to_owned(),
            });
        }
        Ok(value)
    }

    fn get_json_page(
        &self,
        url: &str,
        headers: &[(&str, String)],
    ) -> Result<(Value, Option<String>), GitHubDeviceError> {
        let response = self
            .transport
            .request("GET", url, headers, None)
            .map_err(GitHubDeviceError::Transport)?;
        let value: Value = serde_json::from_str(&response.body)
            .map_err(|error| GitHubDeviceError::Invalid(error.to_string()))?;
        if !(200..300).contains(&response.status) {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or(&response.body);
            let saml = response.headers.contains_key("x-github-sso");
            return Err(GitHubDeviceError::Http {
                status: response.status,
                message: if saml {
                    format!(
                        "{message}; authorize the GitHub App for your SAML organization and try again"
                    )
                } else if response.status == 401 {
                    format!("{message}; sign in with GitHub again")
                } else {
                    message.to_owned()
                },
            });
        }
        let next = response
            .headers
            .get("link")
            .and_then(|link| crate::github::next_link(link));
        Ok((value, next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HttpResponse;
    use std::{
        collections::{HashMap, VecDeque},
        sync::Mutex,
    };

    struct Fake {
        responses: Mutex<VecDeque<HttpResponse>>,
        requests: Mutex<Vec<Value>>,
    }
    impl GitHubTransport for Fake {
        fn request(
            &self,
            _: &str,
            _: &str,
            _: &[(&str, String)],
            body: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            self.requests
                .lock()
                .unwrap()
                .push(body.cloned().unwrap_or(Value::Null));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or("missing response".into())
        }
    }
    fn response(body: Value) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn reads_current_login_from_enterprise_user_endpoint() {
        struct UserResponse;
        impl GitHubTransport for UserResponse {
            fn request(
                &self,
                method: &str,
                url: &str,
                headers: &[(&str, String)],
                _: Option<&Value>,
            ) -> Result<HttpResponse, String> {
                assert_eq!(method, "GET");
                assert_eq!(url, "https://ghe.test/api/v3/user");
                assert!(headers.iter().any(|(name, value)| {
                    *name == "Authorization" && value == "Bearer secret-token"
                }));
                Ok(response(json!({"login":"alice"})))
            }
        }
        let client = GitHubDeviceClient::new("client", "https://ghe.test", Arc::new(UserResponse));
        assert_eq!(client.current_login("secret-token").unwrap(), "alice");
    }

    #[test]
    fn handles_device_protocol_states_and_refresh_tokens() {
        let fake = Arc::new(Fake {
            responses: Mutex::new(VecDeque::from([
                response(
                    json!({"device_code":"device","user_code":"ABCD-EFGH","verification_uri":"https://github.test/login/device","expires_in":900,"interval":5}),
                ),
                response(json!({"error":"authorization_pending"})),
                response(json!({"error":"slow_down"})),
                response(
                    json!({"access_token":"access","refresh_token":"refresh","expires_in":28800,"refresh_token_expires_in":15897600,"token_type":"bearer","scope":"repo"}),
                ),
                response(
                    json!({"access_token":"next","refresh_token":"next-refresh","expires_in":28800}),
                ),
                response(json!({"installations":[{"id":42}]})),
                response(
                    json!({"repositories":[{"full_name":"small-tale/hotsheet2"},{"full_name":"small-tale/docs"}]}),
                ),
            ])),
            requests: Mutex::new(vec![]),
        });
        let client = GitHubDeviceClient::new("client", "https://github.test", fake.clone());
        assert_eq!(client.start().unwrap().user_code, "ABCD-EFGH");
        assert_eq!(client.poll("device").unwrap(), DevicePoll::Pending);
        assert_eq!(client.poll("device").unwrap(), DevicePoll::SlowDown);
        let DevicePoll::Authorized(bundle) = client.poll("device").unwrap() else {
            panic!()
        };
        assert_eq!(bundle.refresh_token.as_deref(), Some("refresh"));
        assert_eq!(client.refresh("refresh").unwrap().access_token, "next");
        assert_eq!(
            client.installed_repositories("access").unwrap(),
            vec!["small-tale/docs", "small-tale/hotsheet2"]
        );
        assert!(fake.requests.lock().unwrap()[0].get("scope").is_none());
    }

    #[test]
    fn repository_access_follows_pagination_and_reports_installation_grants() {
        // HS2-27T5WT: users with many repositories saw one page (or one installation's picks).
        let page = |body: Value, next: Option<&str>| HttpResponse {
            status: 200,
            headers: next
                .map(|url| HashMap::from([("link".to_string(), format!("<{url}>; rel=\"next\""))]))
                .unwrap_or_default(),
            body: body.to_string(),
        };
        let fake = Arc::new(Fake {
            responses: Mutex::new(VecDeque::from([
                page(
                    json!({"installations":[{"id":1,"app_slug":"hot-sheet","repository_selection":"selected","html_url":"https://github.test/settings/installations/1","account":{"login":"westphal"}}]}),
                    Some("https://api.test/user/installations?page=2"),
                ),
                page(
                    json!({"installations":[{"id":2,"repository_selection":"all","account":{"login":"Small-Tale"}}]}),
                    None,
                ),
                page(
                    json!({"repositories":[{"full_name":"westphal/Zeta"}]}),
                    Some("https://api.test/user/installations/1/repositories?page=2"),
                ),
                page(
                    json!({"repositories":[{"full_name":"westphal/alpha"}]}),
                    None,
                ),
                page(
                    json!({"repositories":[{"full_name":"Small-Tale/hotsheet2"},{"full_name":"westphal/alpha"}]}),
                    None,
                ),
            ])),
            requests: Mutex::new(vec![]),
        });
        let client = GitHubDeviceClient::new("client", "https://github.test", fake.clone());
        let access = client.repository_access("token").unwrap();
        assert_eq!(
            access.repositories,
            ["Small-Tale/hotsheet2", "westphal/alpha", "westphal/Zeta"]
        );
        assert_eq!(
            access.installations,
            [
                AppInstallation {
                    account: "westphal".into(),
                    selection: "selected".into(),
                    settings_url: Some("https://github.test/settings/installations/1".into()),
                },
                AppInstallation {
                    account: "Small-Tale".into(),
                    selection: "all".into(),
                    settings_url: None,
                },
            ]
        );
        assert_eq!(
            access.install_url.as_deref(),
            Some("https://github.test/apps/hot-sheet/installations/new")
        );
        assert!(
            fake.responses.lock().unwrap().is_empty(),
            "every page was read"
        );
    }

    #[test]
    fn reports_expiry_denial_and_missing_configuration() {
        let fake = Arc::new(Fake {
            responses: Mutex::new(VecDeque::from([
                response(json!({"error":"expired_token"})),
                response(json!({"error":"access_denied"})),
            ])),
            requests: Mutex::new(vec![]),
        });
        let client = GitHubDeviceClient::new("client", "https://github.test", fake);
        assert_eq!(client.poll("one").unwrap(), DevicePoll::Expired);
        assert_eq!(client.poll("two").unwrap(), DevicePoll::Denied);
        assert!(matches!(
            GitHubDeviceClient::live("", "https://github.com").start(),
            Err(GitHubDeviceError::MissingClientId)
        ));
    }
}
