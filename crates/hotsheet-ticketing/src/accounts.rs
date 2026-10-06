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
use crate::secrets::{KeyMetadata, KeyRegistry, SecretError, SecretStore};

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
    /// The provider host signed in to (`github.com`, a GitHub Enterprise or Jira site). For a
    /// Hot Sheet GitHub sign-in no source uses yet it comes from the site recorded in
    /// `keys.json` (HS2-16MYXN); empty only when no site is known at all.
    pub host: String,
    /// The endpoint setting a new source reusing this account needs (HS2-16MYXN,
    /// HS2-F5HNJN): GitHub Enterprise or self-managed GitLab `api_base`, or the Jira site
    /// `base_url`. Absent for the provider's public default (github.com, gitlab.com).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
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

/// The endpoint setting a connection carries: `api_base` (GitHub, GitLab) or the Jira site.
fn connection_base_url(connection: &ProviderConnection) -> Option<String> {
    let key = if connection.provider == "jira" {
        "base_url"
    } else {
        "api_base"
    };
    setting(connection, key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// The GitHub `api_base` for a sign-in's web origin: none for github.com, `{site}/api/v3`
/// for GitHub Enterprise Server.
pub fn github_api_base_for_site(site: &str) -> Option<String> {
    let site = site.trim().trim_end_matches('/');
    (!site.is_empty() && host_of(site) != "github.com").then(|| format!("{site}/api/v3"))
}

/// The `api_base` a new GitHub source reusing `credential` needs, from the sign-in site
/// recorded in `keys.json` (HS2-16MYXN). `None` for github.com or an unknown site.
pub fn reused_github_api_base(credential: &str, credentials: &[KeyMetadata]) -> Option<String> {
    credentials
        .iter()
        .find(|key| key.provider == credential)
        .and_then(|key| key.site.as_deref())
        .and_then(github_api_base_for_site)
}

/// Fill a new GitHub connection's missing `api_base` from the reused sign-in's recorded site,
/// so a GitHub Enterprise account reused by another project never falls back to github.com
/// (HS2-16MYXN). Connections that already carry an `api_base`, other providers, and
/// github.com sign-ins are left unchanged.
pub fn fill_reused_github_api_base(
    connection: &mut ProviderConnection,
    credentials: &[KeyMetadata],
) {
    if connection.provider != "github" || connection_base_url(connection).is_some() {
        return;
    }
    let Some(api_base) = credential_of(connection)
        .and_then(|credential| reused_github_api_base(credential, credentials))
    else {
        return;
    };
    if let serde_json::Value::Object(settings) = &mut connection.settings {
        settings.insert("api_base".into(), api_base.into());
    }
}

/// Derive every account from the connection records, the checkout links, and the credentials
/// the key registry knows (`keys.json`). A Hot Sheet GitHub sign-in no connection uses yet is
/// listed too, with the host of its recorded site, so it can be reused or signed out; other
/// unused keys (AI provider keys, unrelated tokens) are not accounts and are left out.
pub fn list_accounts(
    connections: &[ProviderConnection],
    checkouts: &[Checkout],
    credentials: &[KeyMetadata],
) -> Vec<Account> {
    let mut accounts: BTreeMap<String, Account> = BTreeMap::new();
    let identities = credentials
        .iter()
        .filter_map(|key| {
            key.identity
                .as_deref()
                .map(|identity| (key.provider.as_str(), identity))
        })
        .collect::<BTreeMap<_, _>>();
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
                base_url: None,
                identity: identities.get(credential).map(|value| (*value).to_owned()),
                managed: credential.starts_with(MANAGED_CREDENTIAL_PREFIX),
                sources: Vec::new(),
                projects: Vec::new(),
            });
        if account.identity.is_none() {
            account.identity = setting(connection, "email").map(str::to_owned);
        }
        if account.base_url.is_none() {
            account.base_url = connection_base_url(connection);
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
    for credential in credentials {
        let name = &credential.provider;
        if name.starts_with(MANAGED_CREDENTIAL_PREFIX) && !accounts.contains_key(name) {
            let site = credential.site.as_deref().unwrap_or_default();
            accounts.insert(
                name.clone(),
                Account {
                    id: name.clone(),
                    provider: "github".into(),
                    host: host_of(site),
                    base_url: github_api_base_for_site(site),
                    identity: credential.identity.clone(),
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
    #[error("ticket source '{connection_id}' is not signed in through account '{account}'")]
    SourceNotFound {
        account: String,
        connection_id: String,
    },
    #[error("ticket source '{connection_id}' is still used by {}", .projects.join(", "))]
    SourceInUse {
        connection_id: String,
        projects: Vec<String>,
    },
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Checkout(#[from] CheckoutError),
    #[error(transparent)]
    Secret(#[from] SecretError),
}

/// Remove an account's unused external source from an in-memory connection catalog. The caller
/// persists the returned catalog; this never unlinks projects or deletes the account credential.
pub fn take_unused_source(
    connections: &mut Vec<ProviderConnection>,
    checkouts: &[Checkout],
    account: &str,
    source: &str,
) -> Result<ProviderConnection, AccountError> {
    let Some(index) = connections.iter().position(|connection| {
        connection.id == source
            && connection.provider != "git"
            && credential_of(connection) == Some(account)
    }) else {
        return Err(AccountError::SourceNotFound {
            account: account.into(),
            connection_id: source.into(),
        });
    };
    let projects = connection_projects(checkouts, source)
        .into_iter()
        .map(|project| project.alias)
        .collect::<Vec<_>>();
    if !projects.is_empty() {
        return Err(AccountError::SourceInUse {
            connection_id: source.into(),
            projects,
        });
    }
    Ok(connections.remove(index))
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

    fn key(name: &str, site: Option<&str>) -> KeyMetadata {
        KeyMetadata {
            provider: name.into(),
            env: crate::secrets::env_name(name),
            site: site.map(str::to_owned),
            identity: None,
        }
    }

    #[test]
    fn github_login_labels_both_used_and_unused_sign_ins() {
        let mut used = key("github-app-used", Some("https://github.com"));
        used.identity = Some("alice".into());
        let mut unused = key("github-app-unused", Some("https://github.com"));
        unused.identity = Some("bob".into());
        let accounts = list_accounts(
            &[external(
                "github-source",
                "github",
                "alice/repo",
                "github-app-used",
                serde_json::json!({}),
            )],
            &[],
            &[used, unused],
        );
        assert_eq!(
            accounts
                .iter()
                .map(|account| account.identity.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("bob"), Some("alice")]
        );
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
    fn only_an_unused_source_of_the_named_account_can_be_removed() {
        let legacy = legacy_install();
        let mut connections = legacy.providers.load().unwrap();
        let checkouts = legacy.checkouts.list().unwrap();
        assert!(matches!(
            take_unused_source(
                &mut connections,
                &checkouts,
                "github-app-01aaa",
                "github-acme-old"
            ),
            Err(AccountError::SourceNotFound { .. })
        ));
        assert!(matches!(
            take_unused_source(
                &mut connections,
                &checkouts,
                "github-app-01aaa",
                "github-acme-shared"
            ),
            Err(AccountError::SourceInUse { .. })
        ));
        assert_eq!(connections.len(), 4);
        let removed = take_unused_source(
            &mut connections,
            &checkouts,
            "github-app-01bbb",
            "github-acme-old",
        )
        .unwrap();
        assert_eq!(removed.id, "github-acme-old");
        assert_eq!(connections.len(), 3);
        assert!(
            checkouts
                .iter()
                .all(|checkout| checkout.source(&removed.id).is_none())
        );
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
                key("github-app-01aaa", None),
                key("github-app-01ccc", None),
                key("anthropic", None),
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
        let bases = accounts
            .iter()
            .map(|account| account.base_url.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(bases, [Some("https://ghe.corp.test/api/v3"), None]);
    }

    #[test]
    fn reusable_accounts_report_the_endpoint_a_new_source_needs() {
        let accounts = list_accounts(
            &[
                external(
                    "gl-corp",
                    "gitlab",
                    "team/app",
                    "gl-corp-token",
                    serde_json::json!({"api_base": "https://gitlab.corp.test/api/v4"}),
                ),
                external(
                    "jira-eng",
                    "jira",
                    "ENG",
                    "jira-token",
                    serde_json::json!({"email": "dev@acme.test", "base_url": "https://acme.atlassian.net"}),
                ),
            ],
            &[],
            &[],
        );
        let summary = accounts
            .iter()
            .map(|account| {
                (
                    account.id.as_str(),
                    account.host.as_str(),
                    account.base_url.as_deref(),
                    account.identity.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            [
                (
                    "gl-corp-token",
                    "gitlab.corp.test",
                    Some("https://gitlab.corp.test/api/v4"),
                    None
                ),
                (
                    "jira-token",
                    "acme.atlassian.net",
                    Some("https://acme.atlassian.net"),
                    Some("dev@acme.test")
                ),
            ]
        );
    }

    #[test]
    fn an_unused_sign_in_reports_the_host_of_its_recorded_site() {
        let accounts = list_accounts(
            &[],
            &[],
            &[
                key("github-app-01dot", Some("https://github.com")),
                key("github-app-01ghe", Some("https://ghe.corp.test")),
                key("github-app-01old", None),
            ],
        );
        let summary = accounts
            .iter()
            .map(|account| {
                (
                    account.id.as_str(),
                    account.host.as_str(),
                    account.base_url.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            [
                // Only a sign-in recorded before sites existed (and never backfilled) is unknown.
                ("github-app-01old", "", None),
                (
                    "github-app-01ghe",
                    "ghe.corp.test",
                    Some("https://ghe.corp.test/api/v3")
                ),
                ("github-app-01dot", "github.com", None),
            ]
        );
    }

    #[test]
    fn a_reused_enterprise_sign_in_fills_the_new_sources_api_base() {
        let credentials = [
            key("github-app-01ghe", Some("https://ghe.corp.test/")),
            key("github-app-01dot", Some("https://github.com")),
        ];
        let mut reused = external(
            "",
            "github",
            "corp/app",
            "github-app-01ghe",
            serde_json::json!({}),
        );
        fill_reused_github_api_base(&mut reused, &credentials);
        assert_eq!(reused.settings["api_base"], "https://ghe.corp.test/api/v3");
        // An explicit api_base, a github.com sign-in, an unknown credential, and another
        // provider are left alone.
        let mut explicit = external(
            "",
            "github",
            "corp/app",
            "github-app-01ghe",
            serde_json::json!({"api_base": "https://proxy.corp.test/api/v3"}),
        );
        fill_reused_github_api_base(&mut explicit, &credentials);
        assert_eq!(
            explicit.settings["api_base"],
            "https://proxy.corp.test/api/v3"
        );
        for (provider, credential) in [
            ("github", "github-app-01dot"),
            ("github", "github-app-unknown"),
            ("gitlab", "github-app-01ghe"),
        ] {
            let mut untouched = external("", provider, "a/b", credential, serde_json::json!({}));
            fill_reused_github_api_base(&mut untouched, &credentials);
            assert!(
                untouched.settings.get("api_base").is_none(),
                "{provider} {credential}"
            );
        }
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
