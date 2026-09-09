use std::collections::HashMap;

pub const BUNDLED_GITHUB_DOT_COM_CLIENT_ID: &str = env!("HOTSHEET_BUNDLED_GITHUB_APP_CLIENT_ID");

pub fn client_id_for_web_base(web_base: &str) -> Result<String, String> {
    let origin = normalize_web_base(web_base)?;
    let github_dot_com_override = std::env::var("HOTSHEET_GITHUB_APP_CLIENT_ID").ok();
    if origin == "https://github.com" {
        return resolve_client_id(&origin, github_dot_com_override.as_deref(), None);
    }
    let enterprise = std::env::var("HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS")
        .ok()
        .map(|value| parse_enterprise_client_ids(&value))
        .transpose()?;
    resolve_client_id(
        &origin,
        github_dot_com_override.as_deref(),
        enterprise.as_ref(),
    )
}

fn resolve_client_id(
    web_base: &str,
    github_dot_com_override: Option<&str>,
    enterprise: Option<&HashMap<String, String>>,
) -> Result<String, String> {
    let origin = normalize_web_base(web_base)?;
    if origin == "https://github.com" {
        if let Some(value) = github_dot_com_override {
            if !valid_client_id(value) {
                return Err(
                    "HOTSHEET_GITHUB_APP_CLIENT_ID is not a valid GitHub App Client ID".into(),
                );
            }
            return Ok(value.to_owned());
        }
        return Ok(BUNDLED_GITHUB_DOT_COM_CLIENT_ID.to_owned());
    }
    enterprise
        .and_then(|values| values.get(&origin))
        .filter(|value| valid_client_id(value))
        .cloned()
        .ok_or_else(|| {
            format!(
                "GitHub App sign-in is not configured for {origin}. Register a separate GitHub App on that GitHub Enterprise host, enable Device Flow, and add its public Client ID to HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS."
            )
        })
}

fn parse_enterprise_client_ids(value: &str) -> Result<HashMap<String, String>, String> {
    let configured: HashMap<String, String> = serde_json::from_str(value).map_err(|error| {
        format!(
            "HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS must be a JSON object mapping GitHub web origins to public Client IDs: {error}"
        )
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
    fn github_dot_com_uses_the_bundled_public_client_id() {
        let resolved = resolve_client_id("https://github.com/", None, None).unwrap();
        assert!(valid_client_id(&resolved));
    }

    #[test]
    fn github_dot_com_allows_a_development_override() {
        assert_eq!(
            resolve_client_id("https://github.com", Some("IvDevelopment123"), None).unwrap(),
            "IvDevelopment123"
        );
        assert!(resolve_client_id("https://github.com", Some("bad"), None).is_err());
    }

    #[test]
    fn enterprise_hosts_require_their_own_registered_app() {
        let values =
            parse_enterprise_client_ids(r#"{"https://github.example.test/":"IvEnterprise123"}"#)
                .unwrap();
        assert_eq!(
            resolve_client_id("https://github.example.test", None, Some(&values)).unwrap(),
            "IvEnterprise123"
        );
        assert!(
            resolve_client_id("https://other.example.test", None, Some(&values))
                .unwrap_err()
                .contains("Register a separate GitHub App")
        );
    }

    #[test]
    fn malformed_enterprise_configuration_fails_clearly() {
        assert!(
            parse_enterprise_client_ids("not-json")
                .unwrap_err()
                .contains("JSON object")
        );
        assert!(normalize_web_base("github.example.test").is_err());
        assert!(normalize_web_base("https://github.example.test/path").is_err());
    }
}
