//! `resource renew|release --server <url>`: a task with no shared `HOTSHEET_HOME` and no server
//! secret, such as one inside a guest VM, keeps or ends its own claim through a Hot Sheet
//! server's renewal-token route (HS2-7H1Y4E, `docs/36-resource-claims.md` §36.11).
//!
//! The renewal token is the only credential and travels only as the `Authorization: Bearer`
//! header; the server secret is never read or sent on this path. The URL must be `https://`
//! (plain `http://` only to a loopback address, or anywhere with `--allow-insecure-http` for a
//! host-only network that no other machine can reach). A server certificate is checked against
//! the public web PKI, or, with `--ca-cert`, against exactly that authority. The token never
//! appears in output or in an error.

use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use hotsheet_resource_claims::{TOKEN_ROUTE_PREFIX, parse_renewal_token};
use serde_json::{Value, json};

/// A server URL: `https://host[:port]` (a trailing `/` is allowed). Plain `http://` is allowed
/// to a loopback address, or to any host with `allow_insecure_http`. Credentials, a path, a
/// query and a fragment are refused, since the token travels in a header only.
pub(crate) fn validate_server(server: &str, allow_insecure_http: bool) -> Result<String> {
    let trimmed = server.trim_end_matches('/');
    let (scheme, rest) = trimmed
        .split_once("://")
        .ok_or_else(|| anyhow!("invalid: --server must be https://<host>[:<port>]"))?;
    if rest.is_empty()
        || rest.contains(['/', '?', '#', '@'])
        || trimmed.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        bail!(
            "invalid: --server is a bare https://<host>[:<port>], without credentials, a path, a query or a fragment"
        );
    }
    match scheme {
        "https" => {}
        "http" => {
            let host = rest.rsplit_once(':').map_or(rest, |(host, port)| {
                if port.bytes().all(|b| b.is_ascii_digit()) {
                    host
                } else {
                    rest
                }
            });
            let loopback = matches!(host, "127.0.0.1" | "localhost" | "[::1]");
            if !loopback && !allow_insecure_http {
                bail!(
                    "invalid: --server is plain http:// to a non-loopback address, so the renewal token would cross the network in clear text; use https:// (see docs/36 section 36.11), or pass --allow-insecure-http for a host-only network no other machine can reach"
                );
            }
        }
        _ => bail!("invalid: --server must be https://<host>[:<port>]"),
    }
    Ok(trimmed.to_owned())
}

fn tls_config(ca_cert: Option<&Path>) -> Result<rustls::ClientConfig> {
    let roots = match ca_cert {
        Some(path) => {
            let pem = std::fs::read(path)
                .with_context(|| format!("invalid: reading --ca-cert {}", path.display()))?;
            let mut roots = rustls::RootCertStore::empty();
            for certificate in rustls_pemfile::certs(&mut BufReader::new(pem.as_slice())) {
                let certificate = certificate
                    .map_err(|_| anyhow!("invalid: --ca-cert is not a PEM certificate"))?;
                roots.add(certificate).map_err(|_| {
                    anyhow!("invalid: --ca-cert holds a certificate this client cannot use")
                })?;
            }
            if roots.is_empty() {
                bail!("invalid: --ca-cert holds no certificate");
            }
            roots
        }
        None => rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        },
    };
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| anyhow!("TLS setup: {e}"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

fn clean(text: &str, token: &str) -> String {
    let text = if token.is_empty() {
        text.to_owned()
    } else {
        text.replace(token, "<redacted>")
    };
    text.chars().filter(|c| !c.is_control()).take(400).collect()
}

/// Renew (`duration_ms` given) or release a claim through `server`. Prints the reply and returns
/// an error when the server refuses.
pub(crate) fn call(
    server: &str,
    ca_cert: Option<&Path>,
    allow_insecure_http: bool,
    action: &str,
    target: Option<&str>,
    token: &str,
    duration_ms: Option<u64>,
) -> Result<()> {
    let base = validate_server(server, allow_insecure_http)?;
    let parsed = parse_renewal_token(token).map_err(|e| anyhow!("{}: {e}", e.code()))?;
    if let Some(target) = target
        && parsed.target.to_string() != target
    {
        bail!("invalid: the token is for {}, not {target}", parsed.target);
    }
    // HS2-9ABVDW: an error status is read here, for the server's own code and message.
    let settings = hotsheet_http::config()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(30)));
    let agent = if base.starts_with("https://") {
        hotsheet_http::build_with_tls(settings, Arc::new(tls_config(ca_cert)?))
    } else {
        hotsheet_http::build(settings)
    };
    let url = format!("{base}{TOKEN_ROUTE_PREFIX}/{action}");
    let body = duration_ms.map_or_else(
        || "{}".to_owned(),
        |ms| json!({ "duration_ms": ms }).to_string(),
    );
    let sent = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send(&body);
    let mut response = match sent {
        Ok(response) if response.status().as_u16() >= 400 => {
            let status = response.status().as_u16();
            let reply: Value = response
                .into_body()
                .read_to_string()
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or(Value::Null);
            let code = reply.get("code").and_then(Value::as_str).map_or(
                match status {
                    401 | 403 => "token_invalid",
                    410 => "expired",
                    409 => "not_holder",
                    _ => "backend_unreachable",
                },
                |code| code,
            );
            let message = clean(
                reply
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("the server refused the request"),
                token,
            );
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({ "error": message, "code": code, "status": status })
                )?
            );
            bail!("{code}: {message}");
        }
        Ok(response) => response,
        Err(error) => {
            let text = clean(&error.to_string(), token);
            let hint = if text.to_ascii_lowercase().contains("certificate") {
                "; if the server uses its own certificate, pass --ca-cert <the CA certificate PEM> (hotsheet-cli cert init writes it)"
            } else {
                ""
            };
            bail!("backend_unreachable: could not reach {base}: {text}{hint}");
        }
    };
    let text = response
        .body_mut()
        .read_to_string()
        .context("reading the server's reply")?;
    let reply: Value = serde_json::from_str(&text)
        .map_err(|_| anyhow!("backend_unreachable: the server's reply was not JSON"))?;
    println!("{}", clean_json(&reply, token)?);
    Ok(())
}

/// Pretty-print a reply, refusing to print the token if a misbehaving server echoed it.
fn clean_json(reply: &Value, token: &str) -> Result<String> {
    let text = serde_json::to_string_pretty(reply)?;
    Ok(text.replace(token, "<redacted>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_7h1y4e_server_urls_are_https_or_loopback_or_explicitly_allowed() {
        for ok in [
            "https://host.example:8443",
            "https://10.0.0.5",
            "https://host/",
            "http://127.0.0.1:9000",
            "http://localhost",
            "http://[::1]:1",
        ] {
            assert!(validate_server(ok, false).is_ok(), "{ok}");
        }
        assert_eq!(
            validate_server("https://host:1/", false).unwrap(),
            "https://host:1"
        );
        let refused = validate_server("http://10.0.0.5:7000", false)
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("clear text") && refused.contains("--allow-insecure-http"),
            "{refused}"
        );
        assert!(validate_server("http://10.0.0.5:7000", true).is_ok());
        for bad in [
            "host:8443",
            "ftp://host",
            "https://",
            "https://user:pw@host",
            "https://host/path",
            "https://host?x=1",
            "https://host#f",
            "https://ho st",
        ] {
            assert!(validate_server(bad, true).is_err(), "{bad}");
        }
    }

    #[test]
    fn hs2_7h1y4e_a_pinned_authority_must_be_a_pem_certificate() {
        assert!(tls_config(None).is_ok());
        let dir = tempfile::tempdir().unwrap();
        let missing = tls_config(Some(&dir.path().join("none.pem")))
            .unwrap_err()
            .to_string();
        assert!(missing.contains("--ca-cert"), "{missing}");
        let empty = dir.path().join("empty.pem");
        std::fs::write(&empty, "not a certificate").unwrap();
        assert!(
            tls_config(Some(&empty))
                .unwrap_err()
                .to_string()
                .contains("holds no certificate")
        );
    }

    #[test]
    fn hs2_7h1y4e_a_token_is_never_printed() {
        let token = "hsrc1.abc.def.ghi";
        assert!(!clean(&format!("bad {token} here"), token).contains(token));
        assert!(
            !clean_json(&json!({ "echo": token }), token)
                .unwrap()
                .contains(token)
        );
    }
}
