//! Public GitHub App configuration shared by graphical and headless hosts.

use std::collections::HashMap;

/// Installation links must use the App registered on the selected GitHub origin.
pub fn install_url_for_web_base(bundled_slug: &str, web_base: &str) -> Result<String, String> {
    let origin = normalize_web_base(web_base)?;
    let public = std::env::var("HOTSHEET_GITHUB_APP_SLUG").ok();
    let enterprise = if origin == "https://github.com" {
        None
    } else {
        std::env::var("HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS")
            .ok()
            .map(|value| parse_enterprise_slugs(&value))
            .transpose()?
    };
    resolve_install_url(
        bundled_slug,
        &origin,
        public.as_deref(),
        enterprise.as_ref(),
    )
}

fn resolve_install_url(
    bundled_slug: &str,
    origin: &str,
    public_override: Option<&str>,
    enterprise: Option<&HashMap<String, String>>,
) -> Result<String, String> {
    let slug = if origin == "https://github.com" {
        public_override.unwrap_or(bundled_slug.trim())
    } else {
        enterprise.and_then(|values| values.get(origin)).map(String::as_str).ok_or_else(|| {
            format!("GitHub App installation is not configured for {origin}. Register an App on that host and add its slug to HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS as a JSON object mapping that web origin to its App slug.")
        })?
    };
    if !valid_app_slug(slug) {
        return Err("GitHub App slug must contain lowercase letters, numbers, or hyphens, starting and ending with a letter or number; check HOTSHEET_GITHUB_APP_SLUG or HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS".into());
    }
    Ok(format!("{origin}/apps/{slug}/installations/new"))
}

fn valid_app_slug(value: &str) -> bool {
    let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    !value.is_empty()
        && value.bytes().all(|byte| alphanumeric(byte) || byte == b'-')
        && value.bytes().next().is_some_and(alphanumeric)
        && value.bytes().last().is_some_and(alphanumeric)
}

fn parse_enterprise_slugs(value: &str) -> Result<HashMap<String, String>, String> {
    let configured: HashMap<String, String> = serde_json::from_str(value).map_err(|error| {
        format!("HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS must be a JSON object mapping GitHub web origins to App slugs: {error}")
    })?;
    configured
        .into_iter()
        .map(|(origin, slug)| {
            let origin = normalize_web_base(&origin)?;
            if !valid_app_slug(&slug) {
                return Err(format!("invalid GitHub App slug configured for {origin}"));
            }
            Ok((origin, slug))
        })
        .collect()
}

pub fn client_id_for_web_base(bundled_github_id: &str, web_base: &str) -> Result<String, String> {
    let origin = normalize_web_base(web_base)?;
    let github_override = std::env::var("HOTSHEET_GITHUB_APP_CLIENT_ID").ok();
    let enterprise = if origin == "https://github.com" {
        None
    } else {
        std::env::var("HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS")
            .ok()
            .map(|value| parse_enterprise_client_ids(&value))
            .transpose()?
    };
    resolve_client_id(
        bundled_github_id,
        &origin,
        github_override.as_deref(),
        enterprise.as_ref(),
    )
}

fn resolve_client_id(
    bundled_github_id: &str,
    origin: &str,
    github_override: Option<&str>,
    enterprise: Option<&HashMap<String, String>>,
) -> Result<String, String> {
    if origin == "https://github.com" {
        let id = github_override.unwrap_or(bundled_github_id.trim());
        if !valid_client_id(id) {
            return Err("HOTSHEET_GITHUB_APP_CLIENT_ID is not a valid GitHub App Client ID".into());
        }
        return Ok(id.into());
    }
    enterprise
        .and_then(|values| values.get(origin))
        .cloned()
        .ok_or_else(|| format!("GitHub App sign-in is not configured for {origin}. Register a separate GitHub App on that GitHub Enterprise host, enable Device Flow, and add its public Client ID to HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS."))
}

fn parse_enterprise_client_ids(value: &str) -> Result<HashMap<String, String>, String> {
    let configured: HashMap<String, String> = serde_json::from_str(value).map_err(|error| {
        format!("HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS must be a JSON object mapping GitHub web origins to public Client IDs: {error}")
    })?;
    configured
        .into_iter()
        .map(|(origin, client_id)| {
            let origin = normalize_web_base(&origin)?;
            if !valid_client_id(&client_id) {
                return Err(format!(
                    "invalid GitHub App Client ID configured for {origin}"
                ));
            }
            Ok((origin, client_id))
        })
        .collect()
}

fn normalize_web_base(value: &str) -> Result<String, String> {
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

fn valid_client_id(value: &str) -> bool {
    value.starts_with("Iv")
        && value.len() >= 12
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_fdtj98_installation_urls_use_only_the_selected_origins_app() {
        assert_eq!(
            resolve_install_url("hot-sheet-2\n", "https://github.com", None, None).unwrap(),
            "https://github.com/apps/hot-sheet-2/installations/new"
        );
        assert_eq!(
            resolve_install_url(
                "hot-sheet-2",
                "https://github.com",
                Some("custom-app"),
                None
            )
            .unwrap(),
            "https://github.com/apps/custom-app/installations/new"
        );
        let enterprise =
            parse_enterprise_slugs(r#"{"HTTPS://GITHUB.EXAMPLE.TEST/":"enterprise-app"}"#).unwrap();
        assert_eq!(
            resolve_install_url(
                "hot-sheet-2",
                "https://github.example.test",
                Some("public-only"),
                Some(&enterprise)
            )
            .unwrap(),
            "https://github.example.test/apps/enterprise-app/installations/new"
        );
        assert!(
            resolve_install_url(
                "hot-sheet-2",
                "https://missing.test",
                None,
                Some(&enterprise)
            )
            .unwrap_err()
            .contains("HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS")
        );
        for invalid in [
            "",
            "../app",
            "app/path",
            "app?query",
            "app#fragment",
            "-app",
            "app-",
            "App",
            "app%2fpath",
        ] {
            assert!(!valid_app_slug(invalid), "{invalid}");
            assert!(
                resolve_install_url("hot-sheet-2", "https://github.com", Some(invalid), None)
                    .is_err()
            );
        }
        assert!(parse_enterprise_slugs("not-json").is_err());
        assert!(parse_enterprise_slugs(r#"{"https://github.test/path":"app"}"#).is_err());
        assert!(parse_enterprise_slugs(r#"{"https://github.test":"app/path"}"#).is_err());
        for invalid in [
            "https://github.test?host=other",
            "https://user@github.test",
            "https://github.test#app",
            "https://github.test\\path",
        ] {
            assert!(normalize_web_base(invalid).is_err());
        }
    }

    #[test]
    fn bundled_and_enterprise_ids_obey_the_same_origin_rules() {
        assert_eq!(
            resolve_client_id("IvBundled12345", "https://github.com", None, None).unwrap(),
            "IvBundled12345"
        );
        assert_eq!(
            resolve_client_id(
                "IvBundled12345",
                "https://github.com",
                Some("IvOverride12345"),
                None
            )
            .unwrap(),
            "IvOverride12345"
        );
        assert!(resolve_client_id("bad", "https://github.com", None, None).is_err());
        let enterprise =
            parse_enterprise_client_ids(r#"{"https://github.example.test/":"IvEnterprise123"}"#)
                .unwrap();
        assert_eq!(
            resolve_client_id(
                "IvBundled12345",
                "https://github.example.test",
                None,
                Some(&enterprise)
            )
            .unwrap(),
            "IvEnterprise123"
        );
        assert!(
            resolve_client_id(
                "IvBundled12345",
                "https://other.test",
                None,
                Some(&enterprise)
            )
            .unwrap_err()
            .contains("Register a separate GitHub App")
        );
        assert!(normalize_web_base("github.example.test").is_err());
        assert!(normalize_web_base("https://github.example.test/path").is_err());
        assert!(parse_enterprise_client_ids("not-json").is_err());
    }
}
