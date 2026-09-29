//! Public GitHub App Client ID selection shared by graphical and headless hosts.

use std::collections::HashMap;

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
    let normalized = value.trim().trim_end_matches('/');
    if !normalized.starts_with("https://") && !normalized.starts_with("http://") {
        return Err("GitHub web base must be an absolute http(s) URL".into());
    }
    if normalized[normalized.find("://").unwrap_or(0) + 3..].contains('/') {
        return Err("GitHub web base must be an origin without a path".into());
    }
    Ok(normalized.to_ascii_lowercase())
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
