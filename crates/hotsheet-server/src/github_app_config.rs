//! Server wrapper around the shared GitHub App origin configuration. The build script validates
//! and embeds the public GitHub.com Client ID; the CLI embeds the same source file.

pub const BUNDLED_GITHUB_DOT_COM_CLIENT_ID: &str = env!("HOTSHEET_BUNDLED_GITHUB_APP_CLIENT_ID");

pub fn client_id_for_web_base(web_base: &str) -> Result<String, String> {
    hotsheet_extsync::github_app_config::client_id_for_web_base(
        BUNDLED_GITHUB_DOT_COM_CLIENT_ID,
        web_base,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_client_id_is_valid_for_github_dot_com() {
        assert_eq!(
            client_id_for_web_base("https://github.com/").unwrap(),
            BUNDLED_GITHUB_DOT_COM_CLIENT_ID,
        );
    }
}
