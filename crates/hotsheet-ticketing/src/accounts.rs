//! Machine-wide provider accounts (HS2-SM9PM8).
//!
//! Ticket sources belong to projects: a provider connection is owned by the checkouts that
//! link it in `checkouts.json` (many-to-many), and there is no machine-wide catalog of them.
//! What *is* machine-wide is the sign-in a connection uses — its credential reference
//! (`settings.credential.secret`, held in the OS keychain through [`KeyRegistry`]). This
//! module derives those **accounts** from the connection records and checkout links, so App
//! Settings → Accounts and `hotsheet account list` can show each sign-in with the projects
//! that use it, and a new source in another project can reuse it without signing in again.
//!
//! Accounts are derived, never stored: there is nothing to migrate. Installs that predate
//! project-owned sources keep their `providers.json` and `checkouts.json` unchanged; each
//! connection is owned by the checkouts already linking it, and a connection no checkout
//! links is reported under its account as used by no project.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::checkouts::{Checkout, CheckoutError};
use crate::connection_removal::{MANAGED_CREDENTIAL_PREFIX, credential_of};
use crate::provider::{ProviderConnection, ProviderError};
use crate::secrets::{KeyRegistry, SecretError, SecretStore};

/// A project (checkout) that uses a connection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AccountProject {
    pub id: String,
    pub alias: String,
}

/// One ticket source signed in through an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSource {
    pub connection_id: String,
    pub name: String,
    pub locator: String,
    #[serde(default)]
    pub disabled: bool,
    /// The projects that own this source. Empty for a record no project links.
    pub projects: Vec<AccountProject>,
}

/// A machine-wide sign-in to a ticket provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// The credential reference (OS-keychain entry name). Never the secret itself.
    pub id: String,
    pub provider: String,
    /// The provider host signed in to (`github.com`, a GitHub Enterprise or Jira site).
    /// Empty when unknown, as for a Hot Sheet GitHub sign-in no source uses yet.
    pub host: String,
    /// The account's own identity when the connection records one (a Jira email).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Minted by Hot Sheet's GitHub sign-in (as opposed to a user-registered key).
    pub managed: bool,
    pub sources: Vec<AccountSource>,
    /// Every project using any of this account's sources, deduplicated.
    pub projects: Vec<AccountProject>,
}

/// The checkouts that link `connection_id`, sorted by alias.
pub fn connection_projects(checkouts: &[Checkout], connection_id: &str) -> Vec<AccountProject> {
    let mut projects = checkouts
        .iter()
        .filter(|checkout| checkout.source(connection_id).is_some())
        .map(|checkout| AccountProject {
            id: checkout.id.clone(),
            alias: checkout.alias.clone(),
        })
        .collect::<Vec<_>>();
    projects.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
    projects
}

fn host_of(url: &str) -> String {
    let rest = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    rest.split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn setting<'a>(connection: &'a ProviderConnection, key: &str) -> Option<&'a str> {
    connection
        .settings
        .get(key)
        .and_then(serde_json::Value::as_str)
}

/// The host a connection signs in to.
pub fn connection_host(connection: &ProviderConnection) -> String {
    match connection.provider.as_str() {
        "github" => setting(connection, "api_base")
            .map(|base| host_of(base).trim_start_matches("api.").to_owned())
            .unwrap_or_else(|| "github.com".into()),
        "gitlab" => setting(connection, "api_base")
            .map(host_of)
            .unwrap_or_else(|| "gitlab.com".into()),
        "jira" => setting(connection, "base_url")
            .map(host_of)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Derive every account from the connection records, the checkout links, and the credential
/// names the key registry knows (`keys.json`). A Hot Sheet GitHub sign-in no connection uses
/// yet is listed too, so it can be reused or signed out; other unused keys (AI provider keys,
/// unrelated tokens) are not accounts and are left out.
pub fn list_accounts(
    connections: &[ProviderConnection],
    checkouts: &[Checkout],
    credential_names: &[String],
) -> Vec<Account> {
    let mut accounts: BTreeMap<String, Account> = BTreeMap::new();
    for connection in connections {
        if connection.provider == "git" {
            continue;
        }
        let Some(credential) = credential_of(connection) else {
            continue;
        };
        let projects = connection_projects(checkouts, &connection.id);
        let account = accounts
            .entry(credential.to_owned())
            .or_insert_with(|| Account {
                id: credential.to_owned(),
                provider: connection.provider.clone(),
                host: connection_host(connection),
                identity: None,
                managed: credential.starts_with(MANAGED_CREDENTIAL_PREFIX),
                sources: Vec::new(),
                projects: Vec::new(),
            });
        if account.identity.is_none() {
            account.identity = setting(connection, "email").map(str::to_owned);
        }
        account.sources.push(AccountSource {
            connection_id: connection.id.clone(),
            name: connection
                .name
                .clone()
                .unwrap_or_else(|| connection.id.clone()),
            locator: connection.locator.clone(),
            disabled: connection.disabled,
            projects: projects.clone(),
        });
        account.projects.extend(projects);
    }
    for name in credential_names {
        if name.starts_with(MANAGED_CREDENTIAL_PREFIX) && !accounts.contains_key(name) {
            accounts.insert(
                name.clone(),
                Account {
                    id: name.clone(),
                    provider: "github".into(),
                    host: String::new(),
                    identity: None,
                    managed: true,
                    sources: Vec::new(),
                    projects: Vec::new(),
                },
            );
        }
    }
    let mut accounts = accounts.into_values().collect::<Vec<_>>();
    for account in &mut accounts {
        account
            .projects
            .sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        account.projects.dedup();
        account.sources.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.connection_id.cmp(&b.connection_id))
        });
    }
    accounts.sort_by(|a, b| {
        a.provider
            .cmp(&b.provider)
            .then(a.host.cmp(&b.host))
            .then(a.id.cmp(&b.id))
    });
    accounts
}

#[derive(Debug, Error)]
pub enum AccountError {
    #[error(
        "account '{account}' is still used by {}; remove those ticket sources first",
        describe_users(.sources, .projects)
    )]
    InUse {
        account: String,
        sources: Vec<String>,
        projects: Vec<String>,
    },
    #[error("no account named '{0}'")]
    NotFound(String),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Checkout(#[from] CheckoutError),
    #[error(transparent)]
    Secret(#[from] SecretError),
}

fn describe_users(sources: &[String], projects: &[String]) -> String {
    if projects.is_empty() {
        format!("ticket source {}", sources.join(", "))
    } else {
        format!("{} ({})", projects.join(", "), sources.join(", "))
    }
}

/// Sign out of `account`: delete its credential from the OS keychain and `keys.json`.
/// Refused while any connection record still references it, so signing out never silently
/// breaks a project's ticket source. Shared by the server and `hotsheet account sign-out`.
pub fn sign_out<S: SecretStore>(
    connections: &[ProviderConnection],
    checkouts: &[Checkout],
    keys: &KeyRegistry<S>,
    account: &str,
) -> Result<(), AccountError> {
    let users = connections
        .iter()
        .filter(|connection| credential_of(connection) == Some(account))
        .collect::<Vec<_>>();
    if !users.is_empty() {
        let mut projects = users
            .iter()
            .flat_map(|connection| connection_projects(checkouts, &connection.id))
            .map(|project| project.alias)
            .collect::<Vec<_>>();
        projects.sort();
        projects.dedup();
        return Err(AccountError::InUse {
            account: account.into(),
            sources: users
                .iter()
                .map(|connection| connection.id.clone())
                .collect(),
            projects,
        });
    }
    if keys.delete(account)? {
        Ok(())
    } else {
        Err(AccountError::NotFound(account.into()))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::checkouts::{CheckoutRegistry, TicketSource};
    use crate::provider::ProviderConfigRegistry;

    #[derive(Default)]
    struct Memory(RefCell<HashMap<String, String>>);

    impl SecretStore for Memory {
        fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
            self.0.borrow_mut().insert(account.into(), secret.into());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            Ok(self.0.borrow().get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            Ok(self.0.borrow_mut().remove(account).is_some())
        }
    }

    fn external(
        id: &str,
        provider: &str,
        locator: &str,
        credential: &str,
        extra: serde_json::Value,
    ) -> ProviderConnection {
        let mut settings = serde_json::json!({"credential": {"secret": credential}});
        if let serde_json::Value::Object(extra) = extra {
            for (key, value) in extra {
                settings[key] = value;
            }
        }
        ProviderConnection {
            id: id.into(),
            provider: provider.into(),
            locator: locator.into(),
            name: Some(format!("{provider} {locator}")),
            default: false,
            settings,
            disabled: false,
        }
    }

    fn link(id: &str, provider: &str, locator: &str) -> TicketSource {
        TicketSource {
            connection_id: id.into(),
            provider: provider.into(),
            locator: locator.into(),
        }
    }

    /// A pre-HS2-SM9PM8 install: a machine-wide catalog in `providers.json` with one
    /// connection per project, one shared by two projects, and one linked by none. Loading it
    /// unchanged yields project ownership from the existing links and machine-wide accounts.
    struct Legacy {
        dir: tempfile::TempDir,
        providers: ProviderConfigRegistry,
        checkouts: CheckoutRegistry,
    }

    fn legacy_install() -> Legacy {
        let dir = tempfile::tempdir().unwrap();
        // Written byte-for-byte in the HS2-3SCH1K format (no ownership field anywhere).
        std::fs::write(
            dir.path().join("providers.json"),
            serde_json::json!({"connections": [
                {"id": "github-acme-procurement", "provider": "github", "locator": "acme/procurement",
                 "name": "Procurement issues", "default": false,
                 "settings": {"credential": {"secret": "github-app-01aaa"}}},
                {"id": "github-acme-shared", "provider": "github", "locator": "acme/shared",
                 "name": "Shared issues", "default": false,
                 "settings": {"credential": {"secret": "github-app-01aaa"}}},
                {"id": "github-acme-old", "provider": "github", "locator": "acme/old",
                 "name": "Old issues", "default": false, "disabled": true,
                 "settings": {"credential": {"secret": "github-app-01bbb"}}},
                {"id": "jira-eng", "provider": "jira", "locator": "ENG", "name": null, "default": false,
                 "settings": {"credential": {"secret": "jira-token"}, "email": "dev@acme.test",
                              "base_url": "https://acme.atlassian.net"}}
            ]})
            .to_string(),
        )
        .unwrap();
        let providers = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        let checkouts = CheckoutRegistry::new(dir.path().join("checkouts.json"));
        for (name, sources) in [
            (
                "procurement",
                vec![
                    link("github-acme-procurement", "github", "acme/procurement"),
                    link("github-acme-shared", "github", "acme/shared"),
                ],
            ),
            (
                "domotion",
                vec![
                    link("github-acme-shared", "github", "acme/shared"),
                    link("jira-eng", "jira", "ENG"),
                ],
            ),
        ] {
            let root = dir.path().join(name);
            std::fs::create_dir(&root).unwrap();
            checkouts
                .register_sources(&root, Some(name), None, sources, None)
                .unwrap();
        }
        Legacy {
            dir,
            providers,
            checkouts,
        }
    }

    #[test]
    fn a_legacy_catalog_becomes_project_owned_sources_and_machine_wide_accounts() {
        let legacy = legacy_install();
        let before = std::fs::read(legacy.dir.path().join("providers.json")).unwrap();
        let connections = legacy.providers.load().unwrap();
        let checkouts = legacy.checkouts.list().unwrap();
        let accounts = list_accounts(
            &connections,
            &checkouts,
            &[
                "github-app-01aaa".into(),
                "github-app-01ccc".into(),
                "anthropic".into(),
            ],
        );
        // Reading is migration: nothing was rewritten.
        assert_eq!(
            std::fs::read(legacy.dir.path().join("providers.json")).unwrap(),
            before
        );
        let summary = accounts
            .iter()
            .map(|account| {
                (
                    account.id.as_str(),
                    account.host.as_str(),
                    account
                        .projects
                        .iter()
                        .map(|project| project.alias.as_str())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            vec![
                // An unused Hot Sheet sign-in (abandoned add flow) is still an account.
                ("github-app-01ccc", "", vec![]),
                (
                    "github-app-01aaa",
                    "github.com",
                    vec!["domotion", "procurement"]
                ),
                // The old catalog-only connection is kept and reported as used by no project.
                ("github-app-01bbb", "github.com", vec![]),
                ("jira-token", "acme.atlassian.net", vec!["domotion"]),
            ]
        );
        let github = &accounts[1];
        assert!(github.managed);
        let owners = github
            .sources
            .iter()
            .map(|source| {
                (
                    source.connection_id.as_str(),
                    source
                        .projects
                        .iter()
                        .map(|project| project.alias.as_str())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            owners,
            vec![
                ("github-acme-procurement", vec!["procurement"]),
                ("github-acme-shared", vec!["domotion", "procurement"]),
            ]
        );
        assert!(accounts[2].sources[0].disabled);
        assert_eq!(accounts[3].identity.as_deref(), Some("dev@acme.test"));
        assert!(!accounts[3].managed);
        // A user key that no connection references (an AI key) is not an account.
        assert!(accounts.iter().all(|account| account.id != "anthropic"));
    }

    #[test]
    fn enterprise_and_gitlab_hosts_come_from_their_api_base() {
        let accounts = list_accounts(
            &[
                external(
                    "ghe",
                    "github",
                    "corp/app",
                    "github-app-01e",
                    serde_json::json!({"api_base": "https://ghe.corp.test/api/v3"}),
                ),
                external("gl", "gitlab", "g/p", "gl-token", serde_json::json!({})),
            ],
            &[],
            &[],
        );
        let hosts = accounts
            .iter()
            .map(|account| account.host.as_str())
            .collect::<Vec<_>>();
        assert_eq!(hosts, ["ghe.corp.test", "gitlab.com"]);
    }

    #[test]
    fn signing_out_is_refused_while_a_source_uses_the_account() {
        let legacy = legacy_install();
        let keys = KeyRegistry::new(legacy.dir.path(), Memory::default());
        keys.set("github-app-01aaa", "{}").unwrap();
        keys.set("github-app-01ccc", "{}").unwrap();
        let connections = legacy.providers.load().unwrap();
        let checkouts = legacy.checkouts.list().unwrap();
        let refused = sign_out(&connections, &checkouts, &keys, "github-app-01aaa").unwrap_err();
        match &refused {
            AccountError::InUse {
                projects, sources, ..
            } => {
                assert_eq!(projects, &["domotion", "procurement"]);
                assert_eq!(sources.len(), 2);
            }
            other => panic!("expected InUse, got {other:?}"),
        }
        assert!(refused.to_string().contains("domotion, procurement"));
        assert!(keys.get("github-app-01aaa").is_ok());
        // An unused sign-in signs out; a second attempt reports it is gone.
        sign_out(&connections, &checkouts, &keys, "github-app-01ccc").unwrap();
        assert!(matches!(
            sign_out(&connections, &checkouts, &keys, "github-app-01ccc"),
            Err(AccountError::NotFound(_))
        ));
    }
}
