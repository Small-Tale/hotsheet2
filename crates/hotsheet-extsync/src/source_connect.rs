//! The one validated workflow that creates a provider connection and links it to a project
//! (HS2-65KYVP). The HTTP route `POST /checkouts/{ref}/provider-connections` and the headless
//! `hotsheet provider-connect` command both run it, so a ticket source prepared from a
//! terminal is validated and linked exactly like one created in a client.

use hotsheet_ticketing::accounts::connection_host;
use hotsheet_ticketing::checkouts::{CheckoutError, CheckoutRegistry};
use hotsheet_ticketing::github_endpoint::{
    Compensation, EndpointError, EndpointRegistry, Expected, Intent, Link, LinkObserver,
    Observation, OperationState, Request, State,
};
use hotsheet_ticketing::{ProviderConfigRegistry, ProviderConnection, ProviderError};

use crate::updated_connections;

/// How [`connect_source`] treats a connection that already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectMode {
    /// Always create a new record; a duplicate explicit id is a conflict (the HTTP form).
    Create,
    /// Reuse the record with the same explicit id, or the same provider, locator, and host,
    /// so re-running the same command changes nothing (the idempotent headless path).
    Reconnect,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectSourceError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
    /// The checkout link failed after the typed candidate was conditionally
    /// compensated. A retained pending candidate has a separate recovery command.
    #[error("{0}")]
    Link(#[from] CheckoutError),
}

struct CheckoutLinks<'a>(Option<&'a CheckoutRegistry>);

impl LinkObserver for CheckoutLinks<'_> {
    fn is_linked(&self, connection_id: &str, link: &Link) -> Result<bool, EndpointError> {
        let checkout = self
            .0
            .ok_or(EndpointError::Storage)?
            .resolve(&link.checkout_id)
            .map_err(|_| EndpointError::Storage)?;
        Ok(checkout.source(connection_id).is_some()
            && (!link.make_default || checkout.default_source.as_deref() == Some(connection_id)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverGithubLink {
    Resume,
    Compensate,
}

#[derive(Debug)]
pub enum GithubLinkRecovery {
    Finalized(ProviderConnection),
    Compensated(Compensation),
}

/// Reacquire the exact saved pending operation after a restart. No caller-supplied
/// digest or endpoint assertion becomes authority. Repeated resume of a completed
/// link returns the current completed row; compensation is conditional on the
/// pending candidate and never overwrites an intervening writer.
pub fn recover_github_link(
    providers: &ProviderConfigRegistry,
    checkouts: &CheckoutRegistry,
    connection_id: &str,
    action: RecoverGithubLink,
) -> Result<GithubLinkRecovery, ConnectSourceError> {
    let current = providers
        .load()?
        .into_iter()
        .find(|row| row.id == connection_id)
        .ok_or_else(|| ProviderError::UnknownConnection(connection_id.into()))?;
    let state = State::load(&current)?.ok_or(EndpointError::Conflict)?;
    let operation = state.operation.as_ref().ok_or(EndpointError::Conflict)?;
    let link = operation
        .link
        .as_ref()
        .ok_or(EndpointError::Conflict)?
        .clone();
    let observer = CheckoutLinks(Some(checkouts));
    if operation.state == OperationState::Completed {
        if action == RecoverGithubLink::Resume && observer.is_linked(connection_id, &link)? {
            return Ok(GithubLinkRecovery::Finalized(current));
        }
        return Err(EndpointError::Conflict.into());
    }
    let core = EndpointRegistry::new(providers);
    let proof = core.pending(connection_id, operation.id, &operation.request_sha256)?;
    match action {
        RecoverGithubLink::Resume => {
            checkouts.add_pending_github_connection(providers, &current, &link, &proof)?;
            Ok(GithubLinkRecovery::Finalized(
                core.finalize(&proof, &observer)?,
            ))
        }
        RecoverGithubLink::Compensate => {
            Ok(GithubLinkRecovery::Compensated(core.compensate(&proof)?))
        }
    }
}

/// Issue a reviewed GitHub endpoint decision and, when requested, complete its
/// exact pending checkout link. The caller has already selected the typed intent
/// and bounded site observation outside provider and checkout locks.
pub fn connect_github_source(
    providers: &ProviderConfigRegistry,
    mut connection: ProviderConnection,
    checkout: Option<CheckoutLink<'_>>,
    mode: ConnectMode,
    intent: Intent,
    observation: &Observation,
) -> Result<ProviderConnection, ConnectSourceError> {
    if connection.provider != "github" {
        return Err(EndpointError::Invalid.into());
    }
    let connections = providers.load()?;
    let existing = match mode {
        ConnectMode::Create => None,
        ConnectMode::Reconnect => {
            let match_row = if connection.id.trim().is_empty() {
                connections.iter().find(|row| {
                    row.provider == "github"
                        && row.locator.eq_ignore_ascii_case(&connection.locator)
                })
            } else {
                connections.iter().find(|row| row.id == connection.id)
            };
            if let Some(row) = match_row {
                if row.provider != "github"
                    || !row.locator.eq_ignore_ascii_case(&connection.locator)
                {
                    return Err(EndpointError::Conflict.into());
                }
            }
            match_row.cloned()
        }
    };
    if let Some(existing) = &existing {
        connection.id = existing.id.clone();
        connection.disabled = existing.disabled;
        if connection.name.is_none() {
            connection.name = existing.name.clone();
        }
        if let Some(webhook) = existing.settings.get("webhook").cloned() {
            connection.settings["webhook"] = webhook;
        }
    } else if connection.id.trim().is_empty() {
        connection.id = hotsheet_ticketing::generate_connection_id(
            &connections,
            &connection.provider,
            &connection.locator,
        );
    }
    let wants_global_default =
        connection.default || existing.as_ref().is_some_and(|item| item.default);
    // A link-pending candidate cannot displace another global default without
    // extending its compensation before-image to that other record. Apply the
    // legacy global default only after the exact link has finalized.
    connection.default = existing.as_ref().is_some_and(|item| item.default);
    crate::descriptor(&connection)?;
    let link = checkout
        .as_ref()
        .map(|checkout| {
            checkout
                .registry
                .resolve(checkout.reference)
                .map(|resolved| Link {
                    checkout_id: resolved.id,
                    make_default: checkout.make_default,
                })
        })
        .transpose()?;
    let core = EndpointRegistry::new(providers);
    let request = Request {
        operation_id: hotsheet_model::Ulid::new(),
        expected: existing.as_ref().map(Expected::capture).transpose()?,
        connection,
        intent,
        link: link.clone(),
    };
    let observer = CheckoutLinks(checkout.as_ref().map(|value| value.registry));
    let mutation = core.apply(&request, observation, &observer)?;
    let Some(recovery) = mutation.recovery else {
        return if wants_global_default {
            set_global_default(providers, &mutation.connection.id)
        } else {
            Ok(mutation.connection)
        };
    };
    let checkout = checkout.expect("pending operation has a checkout");
    let link = link.expect("pending operation has a link");
    match checkout.registry.add_pending_github_connection(
        providers,
        &mutation.connection,
        &link,
        &recovery,
    ) {
        Ok(_) => {
            let completed = core.finalize(&recovery, &observer)?;
            if wants_global_default {
                set_global_default(providers, &completed.id)
            } else {
                Ok(completed)
            }
        }
        Err(error) => match core.compensate(&recovery)? {
            hotsheet_ticketing::github_endpoint::Compensation::Retained(_) => {
                Err(EndpointError::Pending.into())
            }
            _ => Err(error.into()),
        },
    }
}

fn set_global_default(
    providers: &ProviderConfigRegistry,
    connection_id: &str,
) -> Result<ProviderConnection, ConnectSourceError> {
    providers
        .update_connection(connection_id, |rows| {
            if !rows.iter().any(|row| row.id == connection_id) {
                return Err(ProviderError::UnknownConnection(connection_id.into()));
            }
            for row in rows {
                row.default = row.id == connection_id;
            }
            Ok(())
        })?
        .ok_or_else(|| ProviderError::UnknownConnection(connection_id.into()).into())
}

/// The checkout a new source is linked to.
pub struct CheckoutLink<'a> {
    pub registry: &'a CheckoutRegistry,
    /// A checkout id, alias, or path.
    pub reference: &'a str,
    /// Also make the source this checkout's default.
    pub make_default: bool,
}

/// Create (or, with [`ConnectMode::Reconnect`], refresh) `connection` in `providers` and link
/// it to `checkout`. An empty id is generated. The provider descriptor validates the
/// connection's locator and settings before anything is written. With a checkout the
/// record's own `default` is left as it was: a project's default lives on its checkout.
/// If linking fails the legacy path attempts restoration; this is not a guarantee against
/// partial publication. Typed conditional compensation/link recovery is pending (HS2-6H8ZY6).
pub fn connect_source(
    providers: &ProviderConfigRegistry,
    mut connection: ProviderConnection,
    checkout: Option<CheckoutLink<'_>>,
    mode: ConnectMode,
) -> Result<ProviderConnection, ConnectSourceError> {
    let connections = providers.load()?;
    let existing = match mode {
        ConnectMode::Create => None,
        ConnectMode::Reconnect => find_existing(&connections, &connection)?.cloned(),
    };
    if let Some(existing) = &existing {
        connection.id = existing.id.clone();
        connection.disabled = existing.disabled;
        if connection.name.is_none() {
            connection.name = existing.name.clone();
        }
    } else if connection.id.trim().is_empty() {
        connection.id = hotsheet_ticketing::generate_connection_id(
            &connections,
            &connection.provider,
            &connection.locator,
        );
    }
    if mode == ConnectMode::Reconnect && connection.name.is_none() {
        // A first headless connect is named after its provider, like the client form.
        connection.name = default_source_name(&connection.provider).map(str::to_owned);
    }
    let existing_default = existing.as_ref().is_some_and(|item| item.default);
    connection.default = if checkout.is_some() {
        existing_default
    } else {
        connection.default || existing_default
    };
    let previous = connections.clone();
    let replacing = existing.as_ref().map(|item| item.id.as_str());
    let connection_id = connection.id.clone();
    let connection = providers
        .update_connection(&connection_id, |current| {
            let updated =
                updated_connections(std::mem::take(current), connection.clone(), replacing)?;
            *current = updated;
            Ok(())
        })?
        .ok_or_else(|| ProviderError::UnknownConnection(connection_id))?;
    if let Some(link) = checkout {
        let linked = link.registry.add_registered_connection(
            providers,
            link.reference,
            &connection,
            link.make_default,
        );
        if let Err(error) = linked {
            restore(providers, &previous, &connection.id);
            return Err(error.into());
        }
    }
    Ok(connection)
}

/// The name the client's setup form gives a new source of `provider`.
pub fn default_source_name(provider: &str) -> Option<&'static str> {
    match provider {
        "github" => Some("GitHub Issues"),
        "gitlab" => Some("GitLab Issues"),
        "jira" => Some("Jira Cloud"),
        _ => None,
    }
}

/// The record a reconnect replaces: the explicit id, or the same provider, locator, and host.
fn find_existing<'a>(
    connections: &'a [ProviderConnection],
    connection: &ProviderConnection,
) -> Result<Option<&'a ProviderConnection>, ProviderError> {
    let conflict = |message: &str| ProviderError::Conflict {
        ticket: connection.id.clone(),
        message: message.into(),
    };
    if !connection.id.trim().is_empty() {
        let Some(existing) = connections.iter().find(|item| item.id == connection.id) else {
            return Ok(None);
        };
        if existing.provider != connection.provider {
            return Err(conflict(
                "connection id is already used by another provider",
            ));
        }
        if !existing.locator.eq_ignore_ascii_case(&connection.locator)
            || connection_host(existing) != connection_host(connection)
        {
            return Err(conflict(
                "an existing connection id cannot be retargeted to another project; create a new connection",
            ));
        }
        return Ok(Some(existing));
    }
    let host = connection_host(connection);
    Ok(connections.iter().find(|item| {
        item.provider == connection.provider
            && item.locator.eq_ignore_ascii_case(&connection.locator)
            && connection_host(item) == host
    }))
}

/// Put back the record `connection_id` as it was in `previous` (or remove a new one).
fn restore(providers: &ProviderConfigRegistry, previous: &[ProviderConnection], id: &str) {
    // Best effort: the caller already reports the link failure.
    let _ = providers.update(|current| {
        let active_webhook = current
            .iter()
            .find(|item| item.id == id)
            .and_then(|item| item.settings.get("webhook"))
            .cloned();
        current.retain(|item| item.id != id);
        if let Some(mut old) = previous.iter().find(|item| item.id == id).cloned() {
            // Setup can complete between the provider update and a failed checkout
            // link. Roll back that provider edit without erasing the newer key ref.
            if let (Some(settings), Some(webhook)) = (old.settings.as_object_mut(), active_webhook)
            {
                settings.insert("webhook".into(), webhook);
            }
            current.push(old);
        }
        current.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct WebhookSecrets;
    impl hotsheet_ticketing::SecretStore for WebhookSecrets {
        fn get(&self, _: &str) -> Result<Option<String>, hotsheet_ticketing::SecretError> {
            Ok(Some("synthetic-hook-bytes".into()))
        }
        fn set(&self, _: &str, _: &str) -> Result<(), hotsheet_ticketing::SecretError> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<bool, hotsheet_ticketing::SecretError> {
            panic!("setup does not delete keys")
        }
    }
    fn webhook_keys(
        fixture: &Fixture,
        reference: &str,
    ) -> hotsheet_ticketing::KeyRegistry<WebhookSecrets> {
        assert!(std::env::var_os(hotsheet_ticketing::secrets::env_name(reference)).is_none());
        let keys = hotsheet_ticketing::KeyRegistry::new(fixture._dir.path(), WebhookSecrets);
        keys.set(reference, "synthetic-hook-bytes").unwrap();
        keys
    }

    fn gitlab(locator: &str, api_base: Option<&str>) -> ProviderConnection {
        let mut settings = serde_json::json!({"credential":{"secret":"gitlab-key"}});
        if let Some(api_base) = api_base {
            settings["api_base"] = api_base.into();
        }
        ProviderConnection {
            id: String::new(),
            provider: "gitlab".into(),
            locator: locator.into(),
            name: None,
            default: false,
            settings,
            disabled: false,
        }
    }

    fn jira(key: &str) -> ProviderConnection {
        ProviderConnection {
            id: String::new(),
            provider: "jira".into(),
            locator: key.into(),
            name: Some("Jira Cloud".into()),
            default: false,
            settings: serde_json::json!({
                "credential":{"secret":"jira-key"},
                "base_url":"https://acme.atlassian.net",
                "email":"dev@example.com"
            }),
            disabled: false,
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        project: tempfile::TempDir,
        providers: ProviderConfigRegistry,
        checkouts: CheckoutRegistry,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let providers = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        let checkouts = CheckoutRegistry::new(dir.path().join("checkouts.json"));
        checkouts
            .register(project.path(), Some("app"), None, Vec::new())
            .unwrap();
        Fixture {
            _dir: dir,
            project,
            providers,
            checkouts,
        }
    }

    #[test]
    fn hs2_6h8zy6_typed_github_connect_links_and_returns_finalized_decision() {
        use hotsheet_ticketing::github_endpoint::{Decision, Endpoint, State};
        let fixture = fixture();
        let connection = ProviderConnection {
            id: "github-synthetic".into(),
            provider: "github".into(),
            locator: "synthetic/issues".into(),
            name: Some("Synthetic Issues".into()),
            default: false,
            settings: serde_json::json!({"credential":{"secret":"synthetic-key"}}),
            disabled: false,
        };
        let completed = connect_github_source(
            &fixture.providers,
            connection,
            Some(CheckoutLink {
                registry: &fixture.checkouts,
                reference: "app",
                make_default: true,
            }),
            ConnectMode::Create,
            Intent::Explicit {
                credential_ref: "synthetic-key".into(),
                endpoint: Endpoint::Public,
            },
            &Observation::Unavailable,
        )
        .unwrap();
        assert!(matches!(
            State::load(&completed).unwrap().unwrap().decision,
            Decision::Explicit {
                endpoint: Endpoint::Public,
                ..
            }
        ));
        assert!(!State::load(&completed).unwrap().unwrap().is_pending());
        assert_eq!(fixture.providers.load().unwrap(), vec![completed.clone()]);
        let checkout = fixture.checkouts.resolve("app").unwrap();
        assert!(checkout.source(&completed.id).is_some());
        assert_eq!(
            checkout.default_source.as_deref(),
            Some(completed.id.as_str())
        );
    }

    #[test]
    fn hs2_6h8zy6_typed_github_connect_compensates_a_refused_default_link() {
        use hotsheet_ticketing::github_endpoint::{Endpoint, Expected, Link, Request};
        let fixture = fixture();
        let old = ProviderConnection {
            id: "github-old".into(),
            provider: "github".into(),
            locator: "synthetic/old".into(),
            name: None,
            default: false,
            settings: serde_json::json!({"credential":{"secret":"synthetic-key"}}),
            disabled: false,
        };
        fixture.providers.save(std::slice::from_ref(&old)).unwrap();
        let checkout = fixture
            .checkouts
            .add_registered_connection(&fixture.providers, "app", &old, true)
            .unwrap();
        let core = EndpointRegistry::new(&fixture.providers);
        core.apply(
            &Request {
                operation_id: hotsheet_model::Ulid::new(),
                expected: Some(Expected::capture(&old).unwrap()),
                connection: old.clone(),
                intent: Intent::Explicit {
                    credential_ref: "synthetic-key".into(),
                    endpoint: Endpoint::Public,
                },
                link: Some(Link {
                    checkout_id: checkout.id.clone(),
                    make_default: false,
                }),
            },
            &Observation::Unavailable,
            &CheckoutLinks(Some(&fixture.checkouts)),
        )
        .unwrap();
        let pending = fixture.providers.load().unwrap();
        let checkout_before = fixture.checkouts.resolve("app").unwrap();
        let new = ProviderConnection {
            id: "github-new".into(),
            locator: "synthetic/new".into(),
            ..old
        };
        let error = connect_github_source(
            &fixture.providers,
            new,
            Some(CheckoutLink {
                registry: &fixture.checkouts,
                reference: "app",
                make_default: true,
            }),
            ConnectMode::Create,
            Intent::Explicit {
                credential_ref: "synthetic-key".into(),
                endpoint: Endpoint::Public,
            },
            &Observation::Unavailable,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ConnectSourceError::Link(CheckoutError::Endpoint(EndpointError::Pending))
        ));
        assert_eq!(fixture.providers.load().unwrap(), pending);
        assert_eq!(fixture.checkouts.resolve("app").unwrap(), checkout_before);
    }

    #[test]
    fn hs2_6h8zy6_typed_global_default_changes_after_both_links_complete() {
        use hotsheet_ticketing::github_endpoint::Endpoint;
        let fixture = fixture();
        let row = |id: &str| ProviderConnection {
            id: id.into(),
            provider: "github".into(),
            locator: format!("synthetic/{id}"),
            name: None,
            default: true,
            settings: serde_json::json!({"credential":{"secret":"synthetic-key"}}),
            disabled: false,
        };
        for id in ["github-first", "github-second"] {
            let saved = connect_github_source(
                &fixture.providers,
                row(id),
                Some(CheckoutLink {
                    registry: &fixture.checkouts,
                    reference: "app",
                    make_default: true,
                }),
                ConnectMode::Create,
                Intent::Explicit {
                    credential_ref: "synthetic-key".into(),
                    endpoint: Endpoint::Public,
                },
                &Observation::Unavailable,
            )
            .unwrap();
            assert!(saved.default);
            let rows = fixture.providers.load().unwrap();
            assert_eq!(rows.iter().filter(|row| row.default).count(), 1);
            assert_eq!(rows.iter().find(|row| row.default).unwrap().id, id);
            assert_eq!(
                fixture
                    .checkouts
                    .resolve("app")
                    .unwrap()
                    .default_source
                    .as_deref(),
                Some(id)
            );
        }
    }

    #[test]
    fn hs2_6h8zy6_saved_pending_link_resumes_after_restart_and_repeats_without_write() {
        use hotsheet_ticketing::github_endpoint::Endpoint;
        let fixture = fixture();
        let checkout_id = fixture.checkouts.resolve("app").unwrap().id;
        let row = ProviderConnection {
            id: "github-recover".into(),
            provider: "github".into(),
            locator: "synthetic/recover".into(),
            name: None,
            default: false,
            settings: serde_json::json!({"credential":{"secret":"synthetic-key"}}),
            disabled: false,
        };
        EndpointRegistry::new(&fixture.providers)
            .apply(
                &Request {
                    operation_id: hotsheet_model::Ulid::new(),
                    expected: None,
                    connection: row,
                    intent: Intent::Explicit {
                        credential_ref: "synthetic-key".into(),
                        endpoint: Endpoint::Public,
                    },
                    link: Some(Link {
                        checkout_id: checkout_id.clone(),
                        make_default: true,
                    }),
                },
                &Observation::Unavailable,
                &CheckoutLinks(Some(&fixture.checkouts)),
            )
            .unwrap();
        assert!(
            fixture
                .checkouts
                .resolve("app")
                .unwrap()
                .source("github-recover")
                .is_none()
        );
        let GithubLinkRecovery::Finalized(completed) = recover_github_link(
            &fixture.providers,
            &fixture.checkouts,
            "github-recover",
            RecoverGithubLink::Resume,
        )
        .unwrap() else {
            panic!("resume must finalize")
        };
        assert!(!State::load(&completed).unwrap().unwrap().is_pending());
        assert_eq!(
            fixture
                .checkouts
                .resolve("app")
                .unwrap()
                .default_source
                .as_deref(),
            Some("github-recover")
        );
        let before = std::fs::read(fixture.providers.path()).unwrap();
        let GithubLinkRecovery::Finalized(repeated) = recover_github_link(
            &fixture.providers,
            &fixture.checkouts,
            "github-recover",
            RecoverGithubLink::Resume,
        )
        .unwrap() else {
            panic!("repeated resume must return completed")
        };
        assert_eq!(repeated, completed);
        assert_eq!(std::fs::read(fixture.providers.path()).unwrap(), before);
    }

    #[test]
    fn hs2_sgq183_failed_relink_keeps_concurrent_webhook_setup() {
        let fixture = fixture();
        let mut before = ProviderConnection {
            id: "github-synthetic".into(),
            provider: "github".into(),
            locator: "synthetic/issues".into(),
            name: Some("Before".into()),
            default: false,
            settings: serde_json::json!({"credential":{"secret":"api-key"}}),
            disabled: false,
        };
        fixture.providers.save(&[before.clone()]).unwrap();
        before.name = Some("Temporary edit".into());
        fixture.providers.save(&[before.clone()]).unwrap();
        let keys = webhook_keys(&fixture, "hook-key");
        crate::configure_webhook(&fixture.providers, &keys, "github-synthetic", "hook-key")
            .unwrap();
        before.name = Some("Before".into());
        restore(&fixture.providers, &[before], "github-synthetic");
        let [restored] = fixture.providers.load().unwrap().try_into().unwrap();
        assert_eq!(restored.name.as_deref(), Some("Before"));
        assert_eq!(restored.settings["webhook"]["secret"], "hook-key");
    }

    #[test]
    fn hs2_6h8zy6_webhook_and_reconnect_return_actual_normalized_records() {
        use hotsheet_ticketing::github_endpoint::{
            Endpoint, EndpointError, EndpointRegistry, Intent, Link, LinkObserver, Observation,
            Request, State,
        };
        struct NoLinks;
        impl LinkObserver for NoLinks {
            fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
                Ok(false)
            }
        }
        let fixture = fixture();
        let raw = ProviderConnection {
            id: "normalized-github".into(),
            provider: "github".into(),
            locator: "synthetic/project".into(),
            name: Some("Before".into()),
            default: false,
            disabled: false,
            settings: serde_json::json!({"credential":{"secret":"synthetic-account"}}),
        };
        let saved = EndpointRegistry::new(&fixture.providers)
            .apply(
                &Request {
                    operation_id: hotsheet_model::Ulid::new(),
                    expected: None,
                    connection: raw.clone(),
                    intent: Intent::Explicit {
                        credential_ref: "synthetic-account".into(),
                        endpoint: Endpoint::Public,
                    },
                    link: None,
                },
                &Observation::Unavailable,
                &NoLinks,
            )
            .unwrap()
            .connection;
        let keys = webhook_keys(&fixture, "synthetic-hook");
        let webhook =
            crate::configure_webhook(&fixture.providers, &keys, &saved.id, "synthetic-hook")
                .unwrap()
                .connection;
        assert_eq!(webhook, fixture.providers.load().unwrap()[0]);
        assert!(State::load(&webhook).unwrap().unwrap().operation.is_none());
        let mut edited = raw;
        edited.name = Some("After".into());
        let returned = connect_source(
            &fixture.providers,
            edited,
            link(&fixture, true),
            ConnectMode::Reconnect,
        )
        .unwrap();
        assert_eq!(returned, fixture.providers.load().unwrap()[0]);
        assert_eq!(returned.settings["webhook"]["secret"], "synthetic-hook");
        assert!(matches!(
            State::load(&returned).unwrap().unwrap().decision,
            hotsheet_ticketing::github_endpoint::Decision::Explicit { .. }
        ));
        assert_eq!(
            fixture
                .checkouts
                .resolve("app")
                .unwrap()
                .default_source
                .as_deref(),
            Some(returned.id.as_str())
        );
    }

    fn link(fixture: &Fixture, make_default: bool) -> Option<CheckoutLink<'_>> {
        Some(CheckoutLink {
            registry: &fixture.checkouts,
            reference: "app",
            make_default,
        })
    }

    #[test]
    fn reconnect_is_idempotent_and_keeps_name_disabled_and_record_default() {
        let fixture = fixture();
        let first = connect_source(
            &fixture.providers,
            gitlab("acme/app", None),
            link(&fixture, true),
            ConnectMode::Reconnect,
        )
        .unwrap();
        assert_eq!(first.id, "gitlab-acme-app");
        assert!(
            !first.default,
            "a project default never lands on the record"
        );
        fixture.providers.set_disabled(&first.id, true).unwrap();
        let mut renamed = gitlab("ACME/app", None);
        renamed.name = Some("Product".into());
        connect_source(
            &fixture.providers,
            renamed,
            link(&fixture, false),
            ConnectMode::Reconnect,
        )
        .unwrap();
        let again = connect_source(
            &fixture.providers,
            gitlab("acme/app", None),
            link(&fixture, false),
            ConnectMode::Reconnect,
        )
        .unwrap();
        let records = fixture.providers.load().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name.as_deref(), Some("Product"));
        assert!(records[0].disabled);
        assert_eq!(again, records[0]);
        let checkout = fixture.checkouts.resolve("app").unwrap();
        assert_eq!(checkout.sources.len(), 1);
        assert_eq!(checkout.default_source.as_deref(), Some("gitlab-acme-app"));
        let _ = &fixture.project;
    }

    #[test]
    fn a_different_host_is_a_different_source_and_ids_cannot_be_retargeted() {
        let fixture = fixture();
        connect_source(
            &fixture.providers,
            gitlab("acme/app", None),
            None,
            ConnectMode::Reconnect,
        )
        .unwrap();
        let other = connect_source(
            &fixture.providers,
            gitlab("acme/app", Some("https://gitlab.acme.test/api/v4")),
            None,
            ConnectMode::Reconnect,
        )
        .unwrap();
        assert_ne!(other.id, "gitlab-acme-app");
        assert_eq!(fixture.providers.load().unwrap().len(), 2);
        let mut retarget = gitlab("acme/elsewhere", None);
        retarget.id = "gitlab-acme-app".into();
        let error =
            connect_source(&fixture.providers, retarget, None, ConnectMode::Reconnect).unwrap_err();
        assert!(
            error.to_string().contains("cannot be retargeted"),
            "{error}"
        );
        let mut cross = jira("APP");
        cross.id = "gitlab-acme-app".into();
        let error =
            connect_source(&fixture.providers, cross, None, ConnectMode::Reconnect).unwrap_err();
        assert!(error.to_string().contains("another provider"), "{error}");
    }

    #[test]
    fn create_mode_rejects_a_duplicate_id_and_invalid_settings_write_nothing() {
        let fixture = fixture();
        let mut first = jira("APP");
        first.id = "jira-app".into();
        connect_source(&fixture.providers, first.clone(), None, ConnectMode::Create).unwrap();
        let error =
            connect_source(&fixture.providers, first, None, ConnectMode::Create).unwrap_err();
        assert!(error.to_string().contains("already exists"), "{error}");
        let mut invalid = jira("OPS");
        invalid.settings = serde_json::json!({"credential":{"secret":"jira-key"}});
        let error =
            connect_source(&fixture.providers, invalid, None, ConnectMode::Create).unwrap_err();
        assert!(error.to_string().contains("settings.base_url"), "{error}");
        let error = connect_source(
            &fixture.providers,
            gitlab("no-namespace", None),
            None,
            ConnectMode::Reconnect,
        )
        .unwrap_err();
        assert!(error.to_string().contains("namespace/project"), "{error}");
        assert_eq!(fixture.providers.load().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_link_restores_the_registry() {
        let fixture = fixture();
        let missing = CheckoutLink {
            registry: &fixture.checkouts,
            reference: "no-such-project",
            make_default: false,
        };
        let error = connect_source(
            &fixture.providers,
            gitlab("acme/app", None),
            Some(missing),
            ConnectMode::Reconnect,
        )
        .unwrap_err();
        assert!(matches!(error, ConnectSourceError::Link(_)), "{error}");
        assert!(fixture.providers.load().unwrap().is_empty());
        // A failed relink of an existing record puts the previous record back.
        let mut named = gitlab("acme/app", None);
        named.name = Some("Before".into());
        connect_source(&fixture.providers, named, None, ConnectMode::Reconnect).unwrap();
        let mut renamed = gitlab("acme/app", None);
        renamed.name = Some("After".into());
        let missing = CheckoutLink {
            registry: &fixture.checkouts,
            reference: "no-such-project",
            make_default: false,
        };
        connect_source(
            &fixture.providers,
            renamed,
            Some(missing),
            ConnectMode::Reconnect,
        )
        .unwrap_err();
        let records = fixture.providers.load().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name.as_deref(), Some("Before"));
    }
}
