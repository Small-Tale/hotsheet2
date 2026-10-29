//! Click-through links into the Hot Sheet client (HS2-04J8WS). The client accepts
//! `/?project=<checkout id|alias|path>&ticket=<slug>` (docs/06, "Project and ticket deep
//! links"); the origin is the configured `link_base`, else the local client the running
//! host recorded in `${HOTSHEET_HOME}/client.json`.

use std::path::Path;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::config::NotificationConfig;

/// Unreserved URL characters stay readable; everything else is encoded.
const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// The client origin links point at, without a trailing slash.
pub fn link_base(home: &Path, config: &NotificationConfig) -> Option<String> {
    let base = config.link_base.clone().or_else(|| {
        let text = std::fs::read_to_string(home.join("client.json")).ok()?;
        let record: serde_json::Value = serde_json::from_str(&text).ok()?;
        record.get("url")?.as_str().map(str::to_owned)
    })?;
    let base = base.trim().trim_end_matches('/');
    (base.starts_with("http://") || base.starts_with("https://")).then(|| base.to_string())
}

/// `{base}/?project=…[&ticket=…]`, each value URL-encoded.
pub fn deep_link(base: &str, project: &str, ticket: Option<&str>) -> String {
    let mut link = format!("{base}/?project={}", utf8_percent_encode(project, QUERY));
    if let Some(ticket) = ticket.filter(|t| !t.is_empty()) {
        link.push_str(&format!("&ticket={}", utf8_percent_encode(ticket, QUERY)));
    }
    link
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_encoded_links() {
        assert_eq!(
            deep_link("http://hs.test:4174", "hotsheet2-ab12", Some("HS2-7")),
            "http://hs.test:4174/?project=hotsheet2-ab12&ticket=HS2-7"
        );
        assert_eq!(
            deep_link("https://hs.test", "/work/my project.hs2", None),
            "https://hs.test/?project=%2Fwork%2Fmy%20project.hs2"
        );
        assert_eq!(
            deep_link("https://hs.test", "a&b", Some("")),
            "https://hs.test/?project=a%26b"
        );
    }

    #[test]
    fn prefers_the_configured_base_then_the_local_client() {
        let home = tempfile::tempdir().unwrap();
        let mut config = NotificationConfig::default();
        assert_eq!(link_base(home.path(), &config), None);
        std::fs::write(
            home.path().join("client.json"),
            r#"{"url":"http://127.0.0.1:4174/","pid":1}"#,
        )
        .unwrap();
        assert_eq!(
            link_base(home.path(), &config).as_deref(),
            Some("http://127.0.0.1:4174")
        );
        config.link_base = Some("https://hotsheet.lan:8443/".into());
        assert_eq!(
            link_base(home.path(), &config).as_deref(),
            Some("https://hotsheet.lan:8443")
        );
        config.link_base = Some("file:///etc".into());
        assert_eq!(
            link_base(home.path(), &config),
            None,
            "only http(s) origins"
        );
        std::fs::write(home.path().join("client.json"), "not json").unwrap();
        config.link_base = None;
        assert_eq!(link_base(home.path(), &config), None);
    }
}
