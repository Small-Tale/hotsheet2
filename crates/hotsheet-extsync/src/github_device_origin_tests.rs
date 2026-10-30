//! HS2-K0D3TJ: normalized device and API origins share the App configuration contract.

use super::*;
use crate::HttpResponse;
use hotsheet_sync::LockExt;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
struct RoutingTransport {
    requests: Mutex<Vec<(String, String)>>,
}

impl GitHubTransport for RoutingTransport {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        _: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        self.requests
            .lock_or_recover()
            .push((method.into(), url.into()));
        if method == "GET" {
            assert!(headers.iter().any(|(name, value)| {
                *name == "Authorization" && value == "Bearer synthetic-token"
            }));
        }
        let value = if url.ends_with("/login/device/code") {
            json!({
                "device_code": "synthetic-device",
                "user_code": "SYNTHETIC",
                "verification_uri": "https://synthetic.example.test/device",
                "expires_in": 900,
                "interval": 5
            })
        } else if url.ends_with("/login/oauth/access_token") {
            json!({"access_token": "synthetic-token"})
        } else if url.ends_with("/user") {
            json!({"login": "synthetic-owner"})
        } else if url.ends_with("/user/installations?per_page=100") {
            json!({"installations": [{
                "id": 7,
                "app_slug": "synthetic-app",
                "repository_selection": "all",
                "account": {"login": "synthetic-owner"}
            }]})
        } else if url.ends_with("/user/installations/7/repositories?per_page=100") {
            json!({"repositories": [{"full_name": "synthetic/repository"}]})
        } else {
            return Err(format!("unexpected synthetic request: {method} {url}"));
        };
        Ok(HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: value.to_string(),
        })
    }
}

#[test]
fn hs2_k0d3tj_device_api_and_installation_urls_share_normalized_origins() {
    for (supplied, web, api) in [
        (
            "https://github.com",
            "https://github.com",
            "https://api.github.com",
        ),
        (
            "HTTPS://GITHUB.COM/",
            "https://github.com",
            "https://api.github.com",
        ),
        (
            " https://GitHub.Com/// ",
            "https://github.com",
            "https://api.github.com",
        ),
        // Explicit ports remain distinct origins, as in the existing App configuration.
        (
            "HTTPS://GITHUB.COM:443/",
            "https://github.com:443",
            "https://github.com:443/api/v3",
        ),
        (
            "HTTPS://GITHUB.COM:8443///",
            "https://github.com:8443",
            "https://github.com:8443/api/v3",
        ),
        (
            "HTTPS://GHE.EXAMPLE.TEST/",
            "https://ghe.example.test",
            "https://ghe.example.test/api/v3",
        ),
        (
            "https://github.com.example.test/",
            "https://github.com.example.test",
            "https://github.com.example.test/api/v3",
        ),
        (
            "HTTP://GITHUB.COM/",
            "http://github.com",
            "http://github.com/api/v3",
        ),
        (
            "HTTP://[::1]:8765/",
            "http://[::1]:8765",
            "http://[::1]:8765/api/v3",
        ),
    ] {
        let fake = Arc::new(RoutingTransport::default());
        let client = GitHubDeviceClient::new("synthetic-client", supplied, fake.clone());
        assert_eq!(client.start().unwrap().device_code, "synthetic-device");
        assert!(matches!(
            client.poll("synthetic-device").unwrap(),
            DevicePoll::Authorized(_)
        ));
        assert_eq!(
            client.refresh("synthetic-refresh").unwrap().access_token,
            "synthetic-token"
        );
        assert_eq!(
            client.current_login("synthetic-token").unwrap(),
            "synthetic-owner"
        );
        let access = client.repository_access("synthetic-token").unwrap();
        assert_eq!(access.repositories, ["synthetic/repository"]);
        assert_eq!(
            *fake.requests.lock_or_recover(),
            [
                ("POST".to_owned(), format!("{web}/login/device/code")),
                ("POST".to_owned(), format!("{web}/login/oauth/access_token")),
                ("POST".to_owned(), format!("{web}/login/oauth/access_token")),
                ("GET".to_owned(), format!("{api}/user")),
                (
                    "GET".to_owned(),
                    format!("{api}/user/installations?per_page=100")
                ),
                (
                    "GET".to_owned(),
                    format!("{api}/user/installations/7/repositories?per_page=100")
                ),
            ],
            "supplied origin: {supplied}"
        );
        assert_eq!(
            access.install_url,
            Some(format!("{web}/apps/synthetic-app/installations/new"))
        );
    }
}

#[test]
fn hs2_k0d3tj_invalid_web_origins_fail_before_any_transport() {
    for invalid in [
        "",
        "https://",
        "github.com",
        "ftp://github.com",
        "https://github.com/path",
        "https://synthetic-user@github.com",
        "https://github.com?query",
        "https://github.com#fragment",
        "https://github.com\\path",
        "https://git hub.test",
    ] {
        let fake = Arc::new(RoutingTransport::default());
        let client = GitHubDeviceClient::new("synthetic-client", invalid, fake.clone());
        for result in [
            client.start().map(|_| ()),
            client.poll("synthetic-device").map(|_| ()),
            client.refresh("synthetic-refresh").map(|_| ()),
            client.current_login("synthetic-token").map(|_| ()),
            client.repository_access("synthetic-token").map(|_| ()),
            client.installed_repositories("synthetic-token").map(|_| ()),
        ] {
            assert!(
                result.unwrap_err().to_string().contains("web base"),
                "{invalid}"
            );
        }
        assert!(fake.requests.lock_or_recover().is_empty(), "{invalid}");
    }
}
