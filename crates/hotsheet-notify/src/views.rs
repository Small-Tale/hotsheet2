//! Saved-view rule filters (HS2-G8NXNC). A rule may name saved views of linked checkouts;
//! the process that notices a ticket change (the server router or a serverless writer) asks
//! [`memberships`] which of those views the ticket is in and records them on the message, so
//! [`crate::Rule::matches`] stays a pure check.

use hotsheet_ticketing::checkouts::Checkout;
use hotsheet_ticketing::saved_views::{self, ViewQuery};

use crate::config::{NotificationConfig, ViewRef};

/// The views enabled rules name, in rule order without repeats.
fn wanted(config: &NotificationConfig) -> Vec<&ViewRef> {
    let mut out: Vec<&ViewRef> = Vec::new();
    for view in config
        .rules
        .iter()
        .filter(|r| r.enabled)
        .flat_map(|r| &r.filter.views)
    {
        if !out.contains(&view) {
            out.push(view);
        }
    }
    out
}

/// Which views named by an enabled rule contain the ticket, among the views of `checkouts`
/// (the checkouts linked to the ticket's store). `contains` tests the ticket against one
/// checkout's view query, typically `|_, q| q.matches(&ticket, now)`; the checkout lets a
/// caller answer `has:commit` from that project's repository. A checkout whose settings
/// cannot be read, or a view that no longer exists, contains nothing.
pub fn memberships(
    config: &NotificationConfig,
    checkouts: &[Checkout],
    contains: impl Fn(&Checkout, &ViewQuery) -> bool,
) -> Vec<ViewRef> {
    let wanted = wanted(config);
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for checkout in checkouts {
        let refs: Vec<_> = wanted
            .iter()
            .filter(|v| v.checkout == checkout.id)
            .collect();
        if refs.is_empty() {
            continue;
        }
        let Ok(views) = saved_views::from_settings(&checkout.settings()) else {
            continue;
        };
        for wanted in refs {
            let matched = views
                .iter()
                .find(|v| v.id.eq_ignore_ascii_case(&wanted.view))
                .is_some_and(|v| contains(checkout, &ViewQuery::parse(&v.query)));
            if matched {
                out.push((*wanted).clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Rule, RuleFilter};
    use crate::message::EventKind;
    use hotsheet_ticketing::saved_views::SavedView;

    fn config(views: Vec<ViewRef>, enabled: bool) -> NotificationConfig {
        NotificationConfig {
            rules: vec![Rule {
                id: "r".into(),
                events: vec![EventKind::TicketStatusChanged],
                channels: vec!["c".into()],
                enabled,
                filter: RuleFilter {
                    views,
                    ..RuleFilter::default()
                },
                cooldown_seconds: 0,
            }],
            ..NotificationConfig::default()
        }
    }

    #[test]
    fn evaluates_only_named_views_of_linked_checkouts() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        let checkout =
            hotsheet_ticketing::checkouts::CheckoutRegistry::new(dir.path().join("checkouts.json"))
                .register(&project, None, None, Vec::new())
                .unwrap();
        let saved = |id: &str, query: &str| SavedView {
            id: id.into(),
            name: id.into(),
            query: query.into(),
        };
        saved_views::replace(
            &checkout.settings(),
            &[saved("yes", "in view"), saved("no", "not in view")],
        )
        .unwrap();
        let named = |view: &str| ViewRef {
            checkout: checkout.id.clone(),
            view: view.into(),
        };
        let asked = std::cell::RefCell::new(Vec::new());
        let contains = |c: &Checkout, q: &ViewQuery| {
            assert_eq!(c.id, checkout.id);
            asked.borrow_mut().push(q.source().to_owned());
            q.source() == "in view"
        };
        let elsewhere = ViewRef {
            checkout: "elsewhere".into(),
            view: "yes".into(),
        };
        let wanted = config(
            vec![named("YES"), named("no"), named("gone"), elsewhere],
            true,
        );
        let linked = std::slice::from_ref(&checkout);
        assert_eq!(memberships(&wanted, linked, contains), vec![named("YES")]);
        assert_eq!(
            *asked.borrow(),
            vec!["in view", "not in view"],
            "a missing view or another checkout's view is never evaluated"
        );
        asked.borrow_mut().clear();
        assert!(memberships(&config(vec![named("yes")], false), linked, contains).is_empty());
        assert!(memberships(&config(Vec::new(), true), linked, contains).is_empty());
        assert!(memberships(&wanted, &[], contains).is_empty());
        assert!(
            asked.borrow().is_empty(),
            "nothing named and linked, nothing evaluated"
        );
    }
}
