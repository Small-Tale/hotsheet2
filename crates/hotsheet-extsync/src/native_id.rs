//! Offline recognition of a provider's native ticket ids (HS2-FXGKBD).
//!
//! An unqualified ticket id in a checkout route can belong to any linked source. Asking a
//! remote provider is a synchronous network call, so the server first checks whether the
//! id even has that provider's shape. The check is purely syntactic: it never builds a
//! client, reads a credential, or contacts the provider.

/// The native id to probe a `provider` source at `locator` with for an unqualified `id`,
/// or `None` when the id cannot name a ticket there and the source must not be asked.
///
/// - `github` / `gitlab`: an issue number (`42`), `#42`, or `<locator>#42` for this
///   source's own repository or project (ASCII case-insensitive); probed as `42`.
/// - `jira`: `<KEY>-<number>` whose key is this source's project key (the locator),
///   ASCII case-insensitive; probed as given.
/// - Any other provider: unknown shape, so `id` is probed unchanged.
pub fn unqualified_native_id(provider: &str, locator: &str, id: &str) -> Option<String> {
    match provider {
        "github" | "gitlab" => issue_number(locator, id),
        "jira" => jira_key(locator, id).then(|| id.to_string()),
        _ => Some(id.to_string()),
    }
}

fn is_number(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

fn issue_number(locator: &str, id: &str) -> Option<String> {
    let number = match id.rsplit_once('#') {
        None => id,
        Some(("", number)) => number,
        Some((repository, number)) if repository.eq_ignore_ascii_case(locator) => number,
        Some(_) => return None,
    };
    is_number(number).then(|| number.to_string())
}

fn jira_key(locator: &str, id: &str) -> bool {
    id.rsplit_once('-').is_some_and(|(key, number)| {
        !locator.is_empty() && key.eq_ignore_ascii_case(locator) && is_number(number)
    })
}

#[cfg(test)]
mod tests {
    use super::unqualified_native_id as shape;

    #[test]
    fn github_and_gitlab_accept_only_issue_numbers_of_their_own_repository() {
        for provider in ["github", "gitlab"] {
            assert_eq!(shape(provider, "acme/repo", "42").as_deref(), Some("42"));
            assert_eq!(shape(provider, "acme/repo", "#42").as_deref(), Some("42"));
            assert_eq!(
                shape(provider, "acme/repo", "Acme/Repo#42").as_deref(),
                Some("42")
            );
            for rejected in [
                "other/repo#42",
                "HS-7F3K9Q",
                "01M4YDRQTJTDNSBW0766QW0J0G",
                "",
                "#",
                "-1",
                "4 2",
                "99999999999999999999999",
                "acme/repo#",
            ] {
                assert_eq!(shape(provider, "acme/repo", rejected), None, "{rejected}");
            }
        }
    }

    #[test]
    fn jira_accepts_only_keys_of_the_configured_project() {
        assert_eq!(shape("jira", "ENG", "ENG-9").as_deref(), Some("ENG-9"));
        assert_eq!(shape("jira", "ENG", "eng-9").as_deref(), Some("eng-9"));
        for rejected in [
            "OPS-9",
            "ENG-",
            "ENG-9A",
            "ENG",
            "9",
            "HS-7F3K9Q",
            "01M4YDRQTJTDNSBW0766QW0J0G",
        ] {
            assert_eq!(shape("jira", "ENG", rejected), None, "{rejected}");
        }
        assert_eq!(shape("jira", "", "ENG-9"), None);
    }

    #[test]
    fn an_unknown_provider_is_probed_with_the_id_unchanged() {
        assert_eq!(
            shape("linear", "team", "HS-7F3K9Q").as_deref(),
            Some("HS-7F3K9Q")
        );
    }
}
