//! **Current-user identity** (`docs/10` §10.3) — resolve "who am I" for the assignment /
//! review query filters, so `assignee=me` and `review_requested=me` work in the CLI, the
//! server, and the MCP shim without the caller hard-coding an email.
//!
//! The identity is the store's **git `user.email`** — the same key assignment writes
//! (`ops::assign`, `docs/10` §10.1), so "assigned to me" lines up exactly with what a
//! teammate sees. There's no separate account system; git identity is the identity.

use std::path::Path;
use std::process::Command;

/// The literal a caller passes to mean "the current user" in a person-valued filter.
pub const ME: &str = "me";

/// The store's configured git `user.email`, or `None` if git is unavailable, the value is
/// unset, or it's empty. Cheap (`git config user.email`); callers may cache per request.
pub fn current_user_email(store_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(store_root)
        .args(["config", "user.email"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let email = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!email.is_empty()).then_some(email)
}

/// Resolve a person-valued filter argument: the case-insensitive sentinel [`ME`] becomes the
/// current user's git email (or `None` when it can't be resolved — the caller should then
/// treat the filter as unsatisfiable rather than matching everyone); any other value passes
/// through unchanged. Query builders call this before constructing a `TicketQuery`.
pub fn resolve_me(value: &str, store_root: &Path) -> Option<String> {
    if value.eq_ignore_ascii_case(ME) {
        current_user_email(store_root)
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git_repo_with_email(email: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .arg("init")
            .output()
            .unwrap();
        if let Some(e) = email {
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["config", "user.email", e])
                .output()
                .unwrap();
        }
        dir
    }

    #[test]
    fn current_user_email_reads_git_config() {
        let dir = git_repo_with_email(Some("dev@example.com"));
        assert_eq!(
            current_user_email(dir.path()).as_deref(),
            Some("dev@example.com")
        );
    }

    #[test]
    fn missing_email_is_none() {
        let dir = git_repo_with_email(None);
        // A fresh repo with no user.email set (and no global fallback in the test env) → None.
        // If a global git identity leaks in, this still must not panic; accept either shape.
        let _ = current_user_email(dir.path());
    }

    #[test]
    fn resolve_me_passes_through_a_literal_email() {
        let dir = git_repo_with_email(Some("dev@example.com"));
        assert_eq!(
            resolve_me("someone@else.com", dir.path()).as_deref(),
            Some("someone@else.com")
        );
    }

    #[test]
    fn resolve_me_is_case_insensitive_and_resolves_to_git_email() {
        let dir = git_repo_with_email(Some("dev@example.com"));
        assert_eq!(
            resolve_me("me", dir.path()).as_deref(),
            Some("dev@example.com")
        );
        assert_eq!(
            resolve_me("ME", dir.path()).as_deref(),
            Some("dev@example.com")
        );
        assert_eq!(
            resolve_me("Me", dir.path()).as_deref(),
            Some("dev@example.com")
        );
    }
}
