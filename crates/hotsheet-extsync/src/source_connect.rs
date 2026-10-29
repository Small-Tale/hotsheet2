//! The one validated workflow that creates a provider connection and links it to a project
//! (HS2-65KYVP). The HTTP route `POST /checkouts/{ref}/provider-connections` and the headless
//! `hotsheet provider-connect` command both run it, so a ticket source prepared from a
//! terminal is validated and linked exactly like one created in a client.

use hotsheet_ticketing::accounts::connection_host;
use hotsheet_ticketing::checkouts::{CheckoutError, CheckoutRegistry, TicketSource};
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
    /// The record was valid but the checkout link could not be written; the registry was
    /// restored to what it held before.
    #[error("{0}")]
    Link(#[from] CheckoutError),
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
/// If the link cannot be written the registry is restored, so no ownerless record is left.
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
    providers.update(|current| {
        let updated = updated_connections(std::mem::take(current), connection.clone(), replacing)?;
        *current = updated;
        Ok(())
    })?;
    if let Some(link) = checkout {
        let linked = link.registry.add_registered_source(
            providers,
            link.reference,
            TicketSource {
                connection_id: connection.id.clone(),
                provider: connection.provider.clone(),
                locator: connection.locator.clone(),
            },
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
        crate::configure_webhook(&fixture.providers, "github-synthetic", "hook-key").unwrap();
        before.name = Some("Before".into());
        restore(&fixture.providers, &[before], "github-synthetic");
        let [restored] = fixture.providers.load().unwrap().try_into().unwrap();
        assert_eq!(restored.name.as_deref(), Some("Before"));
        assert_eq!(restored.settings["webhook"]["secret"], "hook-key");
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
