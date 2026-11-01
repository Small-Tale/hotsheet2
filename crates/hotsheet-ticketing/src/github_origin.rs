//! GitHub web-origin identity shared by saved accounts and protocol clients.

/// HS2-0HE7E4: one origin contract for historical credentials and new requests.
/// Explicit ports remain significant, including an explicit default port.
pub fn normalize_web_base(value: &str) -> Result<String, String> {
    let normalized = value.trim().trim_end_matches('/').to_ascii_lowercase();
    if !normalized.starts_with("https://") && !normalized.starts_with("http://") {
        return Err("GitHub web base must be an absolute http(s) URL".into());
    }
    let authority = &normalized[normalized.find("://").unwrap_or(0) + 3..];
    if authority.is_empty()
        || authority
            .chars()
            .any(|c| c.is_whitespace() || "/\\?#@".contains(c))
    {
        return Err(
            "GitHub web base must be an origin without a path, credentials, query, or fragment"
                .into(),
        );
    }
    Ok(normalized)
}

/// Infer an endpoint only after validating the saved origin. `None` selects the
/// public default; an invalid historical value must never silently select it.
pub fn api_base_for_web_origin(value: &str) -> Result<Option<String>, String> {
    let origin = normalize_web_base(value)?;
    Ok((origin != "https://github.com").then(|| format!("{origin}/api/v3")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_0he7e4_saved_origins_share_public_enterprise_and_port_identity() {
        for (saved, expected) in [
            ("https://github.com", None),
            (" HTTPS://GITHUB.COM/// ", None),
            ("https://GitHub.Com/", None),
            (
                " HTTPS://GHE.EXAMPLE.TEST/// ",
                Some("https://ghe.example.test/api/v3"),
            ),
            (
                "https://github.com:443/",
                Some("https://github.com:443/api/v3"),
            ),
            (
                "https://github.com:8443",
                Some("https://github.com:8443/api/v3"),
            ),
            (
                "https://github.com.example.test",
                Some("https://github.com.example.test/api/v3"),
            ),
            ("HTTP://GITHUB.COM", Some("http://github.com/api/v3")),
            (" HTTP://[::1]:8123/ ", Some("http://[::1]:8123/api/v3")),
        ] {
            assert_eq!(
                api_base_for_web_origin(saved).unwrap().as_deref(),
                expected,
                "{saved}"
            );
        }
    }

    #[test]
    fn hs2_0he7e4_invalid_saved_origins_cannot_select_the_public_default() {
        for saved in [
            "",
            "github.com",
            "ftp://github.com",
            "https://",
            "https://github.com/path",
            "https://user@github.com",
            "https://github.com?query",
            "https://github.com#fragment",
            "https://github.com\\path",
            "https://git hub.com",
        ] {
            assert!(api_base_for_web_origin(saved).is_err(), "{saved:?}");
        }
    }
}
