//! HS2-A8T481 lint: a struct may not hold a secret-named string field (one of
//! `SECRET_WORDS`, such as `token`, `secret`, `csrf` or `authorization`, or a `_`-suffixed
//! form such as `access_token`) as a plain `String`. Such a field uses
//! `hotsheet_sync::secret::SecretString`, whose `Debug` prints `<redacted>`. Otherwise the
//! next `dbg!`, `{:?}` in an error, or failing `assert_eq!` prints the credential.
//!
//! HS2-N1JXBZ widened it from `derive(Debug)` structs to every struct, whatever it derives.
//! A hand-written redacting `Debug` is not enough: a later `derive(Debug)`, a `tracing`
//! field, or `serde_json::to_string` into a log would still print the plain value. A
//! `Serialize` wire or storage type that must carry its secret is not exempt either:
//! `SecretString` is serde-transparent, so it holds the secret with byte-identical JSON and
//! keeps it out of `Debug`. A clap flag holds `SecretString` too, through its `From<String>`.
//! `NOT_SECRET` lists only fields that hold no credential, and test fakes that record a
//! synthetic value they assert on.
//!
//! HS2-CQNE4E extends it to header collections: a `derive(Debug)` struct may not hold a
//! `*header*` field typed as plain strings (`Vec<(String, String)>`, a `String` map, or
//! `HeaderMap`), because `Authorization`, cookie and signature values print with it. Such a
//! struct writes its `Debug` by hand with `hotsheet_sync::secret::RedactedHeaders`, or holds
//! the values as `SecretString`.
//!
//! HS2-7AJ57B covers three more shapes. `key_pem` joins `SECRET_WORDS` for PEM private keys
//! (`client_key_pem`); bare `_key` stays out because map keys and ids would match. Enum
//! struct variants are scanned like structs, and a tuple struct or enum tuple variant whose
//! secret-named type (`ApiToken(String)`, `Bearer(String)`) wraps a plain string is flagged
//! as field `0`. A map value has no field name, so the scan flags the copy instead: an
//! `.insert(...)` whose arguments `.expose()` a `SecretString`.

use std::path::{Path, PathBuf};

/// Fields whose names match but which hold no credential. Each entry is permanent and says why.
const NOT_SECRET: &[(&str, &str, &str, &str)] = &[
    (
        "hotsheet-ticketing/src/provider_outbox.rs",
        "OutboxAdmission",
        "base_token",
        "optimistic-concurrency version, not a credential",
    ),
    (
        "hotsheet-ticketing/src/provider_outbox.rs",
        "OutboxOperation",
        "base_token",
        "optimistic-concurrency version, not a credential",
    ),
    (
        "hotsheet-ticketing/src/provider.rs",
        "ProviderPatch",
        "expected_token",
        "optimistic-concurrency version, not a credential",
    ),
    (
        "hotsheet-ticketing/src/provider.rs",
        "NotWorkingReport",
        "expected_token",
        "optimistic-concurrency version, not a credential",
    ),
    (
        "hotsheet-ticketing/src/wire.rs",
        "ApiTicket",
        "concurrency_token",
        "optimistic-concurrency version, not a credential",
    ),
    (
        "hotsheet-server/src/dto.rs",
        "UpdateReq",
        "expected_token",
        "optimistic-concurrency version, not a credential",
    ),
    (
        "hotsheet-cli/src/orchestration/tickets.rs",
        "HostedUpdate",
        "expected_token",
        "optimistic-concurrency version, not a credential",
    ),
    (
        "hotsheet-cli/src/orchestration/tickets.rs",
        "UpdateTargetTicket",
        "expected_token",
        "optimistic-concurrency version, not a credential",
    ),
    (
        "hotsheet-extsync/src/jira.rs",
        "JiraSearch",
        "next_page_token",
        "Jira pagination cursor, not a credential",
    ),
    (
        "hotsheet-ticketing/src/connection_removal.rs",
        "ConnectionRemoval",
        "kept_credential",
        "OS credential-store account name reported to the user, not the secret (HS2-DGMK06)",
    ),
    (
        "hotsheet-ticketing/src/checkout_removal.rs",
        "CheckoutRemoval",
        "kept_credentials",
        "OS credential-store account names reported to the user, not secrets (HS2-DGMK06)",
    ),
    (
        "hotsheet-ticketing/src/claim_backends.rs",
        "BackendConfiguration",
        "credential",
        "OS credential-store account name; the key itself stays in the store (HS2-DGMK06)",
    ),
    (
        "hotsheet-notify/src/remote_change/registry.rs",
        "Connection",
        "credential",
        "OS credential-store account name; the key itself stays in the store (HS2-DGMK06)",
    ),
    (
        "hotsheet-server/tests/session_messages.rs",
        "Seen",
        "authorization",
        "test fake recording a synthetic header it must assert on (HS2-DGMK06)",
    ),
    (
        "hotsheet-server/tests/resource_remote_cli.rs",
        "Seen",
        "authorization",
        "test fake recording a synthetic header it must assert on (HS2-DGMK06)",
    ),
    (
        "hotsheet-cli/tests/job_ci_watch.rs",
        "Seen",
        "authorization",
        "test fake recording a synthetic header it must assert on (HS2-DGMK06)",
    ),
    (
        "hotsheet-ticketing/src/provider_outbox.rs",
        "Payload",
        "base_token",
        "optimistic-concurrency version, not a credential (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-server/src/routes/providers/tickets.rs",
        "NotWorkingJsonRequest",
        "expected_token",
        "optimistic-concurrency version, not a credential (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-cli/src/dispatch/providers.rs",
        "ProviderEditInput",
        "expected_token",
        "optimistic-concurrency version, not a credential (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-cli/src/dispatch/providers.rs",
        "GithubConnectArgs",
        "credential",
        "clap flag naming an OS credential-store reference, not the secret (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-cli/src/dispatch/providers.rs",
        "ProviderConnectArgs",
        "credential",
        "clap flag naming an OS credential-store reference, not the secret (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-sync/src/secret.rs",
        "Plain",
        "token",
        "plain-String twin that proves SecretString keeps the serde format (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-aitools/tests/opencode_session_push.rs",
        "Seen",
        "authorization",
        "test fake recording a synthetic header it must assert on (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-notify/src/remote_change/tests.rs",
        "FakeAdapter",
        "credentials",
        "test fake recording the synthetic keys it was opened with (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-server/tests/remote_session_lifetime.rs",
        "Paired",
        "cookie",
        "test session holding a synthetic cookie it must replay (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-server/tests/remote_session_lifetime.rs",
        "Paired",
        "csrf",
        "test session holding a synthetic CSRF proof it must replay (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-server/tests/http.rs",
        "DiscardPeerRequest",
        "secret",
        "test fake recording a synthetic peer secret it must assert on (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-mcp/tests/serverless_sync_kick.rs",
        "Received",
        "secret",
        "test fake recording a synthetic machine secret it must assert on (HS2-N1JXBZ)",
    ),
    (
        "hotsheet-notify/src/remote_change/workflow.rs",
        "WorkflowError::Secret",
        "0",
        "OS credential-store error message, not the secret (HS2-7AJ57B)",
    ),
    (
        "hotsheet-server/src/remote_auth.rs",
        "Proof::Browser",
        "csrf",
        "borrowed request-header view compared in place; no Debug or Serialize (HS2-7AJ57B)",
    ),
    (
        "hotsheet-server/tests/remote_host_access.rs",
        "As::Session",
        "cookie",
        "test caller replaying a synthetic cookie (HS2-7AJ57B)",
    ),
    (
        "hotsheet-server/tests/remote_host_access.rs",
        "As::Session",
        "csrf",
        "test caller replaying a synthetic CSRF proof (HS2-7AJ57B)",
    ),
    (
        "hotsheet-cli/src/dispatch/orchestration.rs",
        "TicketCmd::Update",
        "expected_token",
        "optimistic-concurrency version, not a credential (HS2-7AJ57B)",
    ),
    (
        "hotsheet-cli/src/main.rs",
        "Cmd::ProviderEdit",
        "expected_token",
        "optimistic-concurrency version, not a credential (HS2-7AJ57B)",
    ),
    (
        "hotsheet-cli/src/main.rs",
        "Cmd::ProviderReportNotWorking",
        "expected_token",
        "optimistic-concurrency version, not a credential (HS2-7AJ57B)",
    ),
];

// HS2-9ZMC7N retyped the nine secrets this lint found when it landed, so there is no
// baseline: a real secret uses `SecretString` (or a hand-written redacting `Debug`), and
// only a field that holds no credential may be listed in `NOT_SECRET`.

const SELF: &str = "hotsheet-sync/tests/secret_debug_policy.rs";

/// HS2-DGMK06 added the CSRF proof, cookies, bearer and `Authorization` header values, and
/// key material. A `credential` field usually names an OS credential-store account rather
/// than holding the secret, so each such reference is listed in `NOT_SECRET` with its reason.
const SECRET_WORDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "api_key",
    "csrf",
    "cookie",
    "bearer",
    "authorization",
    "credential",
    "credentials",
    "private_key",
    "signing_key",
    "passphrase",
    "jwt",
    // HS2-7AJ57B: a PEM private key (`client_key_pem`); `cert_pem` and `ca_pem` are public.
    "key_pem",
];

#[derive(Debug, PartialEq, Eq)]
struct Finding {
    file: String,
    line: usize,
    strukt: String,
    field: String,
    ty: String,
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_sources(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// `token`, `access_token`, `api_key`: the whole name or its last `_` segment(s).
fn secret_named(field: &str) -> bool {
    SECRET_WORDS
        .iter()
        .any(|word| field == *word || field.ends_with(&format!("_{word}")))
}

/// A header collection of plain strings: `Vec<(String, String)>`, `HashMap<String, String>`,
/// `HeaderMap`, ... in a field named `*header*` (HS2-CQNE4E). `SecretString` values pass.
fn plain_header_collection(field: &str, ty: &str) -> bool {
    field.to_ascii_lowercase().contains("header")
        && !ty.contains("SecretString")
        && (ty.contains("HeaderMap")
            || ((ty.contains("Vec<(") || ty.contains("Map<"))
                && (ty.contains("String") || ty.contains("str"))))
}

/// A plain string type: `String`, `Option<String>`, `&str`, ... but not `SecretString`.
fn plain_string(ty: &str) -> bool {
    !ty.contains("SecretString") && (ty.contains("String") || ty.contains("str"))
}

fn identifier(text: &str) -> &str {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    &text[..end]
}

/// Strip a leading `pub`, `pub(crate)`, or `pub(in ...)` visibility.
fn strip_visibility(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("pub") else {
        return text;
    };
    let rest = rest.trim_start();
    match rest.strip_prefix('(') {
        Some(inner) => inner
            .split_once(')')
            .map_or(rest, |(_, after)| after.trim_start()),
        None => rest,
    }
}

/// Every secret-named plain-string field of any struct in `source`, whatever it derives
/// (HS2-N1JXBZ): a hand-written redacting `Debug` does not cover `Serialize`, a `tracing`
/// field or a later `derive(Debug)`.
fn scan(file: &str, source: &str) -> Vec<Finding> {
    let flagged = |name: &str, ty: &str| secret_named(name) && plain_string(ty);
    let mut findings = scan_fields(file, source, |_| true, flagged);
    findings.extend(scan_unnamed_and_variants(file, source));
    findings.extend(scan_exposed_inserts(file, source));
    findings.sort_by_key(|finding| finding.line);
    findings
}

/// `ApiToken` -> `api_token`, so a type or variant name can be checked like a field name.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(character.to_ascii_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}

/// The text between the first `(` and the last `)` of `item`.
fn tuple_types(item: &str) -> Option<&str> {
    let open = item.find('(')?;
    let close = item.rfind(')')?;
    (close > open).then(|| &item[open + 1..close])
}

/// HS2-7AJ57B: the shapes `scan_fields` cannot see.
/// - A tuple struct whose secret-named type wraps a plain string (`struct ApiToken(String)`).
/// - An enum struct variant's secret-named plain-string field (`Auth::Basic { password }`).
/// - An enum tuple variant whose secret-named variant wraps a plain string (`Bearer(String)`).
///
/// A tuple field reports as `0`; a variant reports as `Enum::Variant`.
fn scan_unnamed_and_variants(file: &str, source: &str) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();
    let finding = |line: usize, strukt: String, field: &str, ty: &str| Finding {
        file: file.to_owned(),
        line,
        strukt,
        field: field.to_owned(),
        ty: ty.trim().trim_end_matches(',').to_owned(),
    };
    let mut i = 0;
    while i < lines.len() {
        let item = strip_visibility(lines[i].trim());
        i += 1;
        if let Some(rest) = item.strip_prefix("struct ") {
            let name = identifier(rest);
            if rest[name.len()..].trim_start().starts_with('(')
                && let Some(types) = tuple_types(rest)
                && secret_named(&snake_case(name))
                && plain_string(types)
            {
                findings.push(finding(i, name.to_owned(), "0", types));
            }
            continue;
        }
        let Some(rest) = item.strip_prefix("enum ") else {
            continue;
        };
        if !item.ends_with('{') {
            continue;
        }
        let enumeration = identifier(rest).to_owned();
        while i < lines.len() && lines[i].trim() != "}" {
            let line = lines[i].trim();
            i += 1;
            if line.starts_with("//") || line.starts_with("#[") {
                continue;
            }
            let variant = identifier(line);
            if variant.is_empty() {
                continue;
            }
            let after = line[variant.len()..].trim_start();
            let strukt = format!("{enumeration}::{variant}");
            if after.starts_with('(') {
                if let Some(types) = tuple_types(after)
                    && secret_named(&snake_case(variant))
                    && plain_string(types)
                {
                    findings.push(finding(i, strukt, "0", types));
                }
            } else if let Some(body) = after.strip_prefix('{') {
                // One-line `Variant { a: String, b: u8 },` or a multi-line body.
                let one_line = body.contains('}');
                let mut fields: Vec<(usize, String)> = Vec::new();
                if one_line {
                    let inner = body.split('}').next().unwrap_or("");
                    fields.extend(inner.split(',').map(|field| (i, field.to_owned())));
                } else {
                    while i < lines.len() && !lines[i].trim().starts_with('}') {
                        fields.push((i + 1, lines[i].trim().to_owned()));
                        i += 1;
                    }
                    i += 1;
                }
                for (line, field) in fields {
                    let field = strip_visibility(field.trim());
                    if let Some((name, ty)) = field.split_once(':') {
                        let name = name.trim();
                        if identifier(name) == name && secret_named(name) && plain_string(ty) {
                            findings.push(finding(line, strukt.clone(), name, ty));
                        }
                    }
                }
            }
        }
    }
    findings
}

/// HS2-7AJ57B: a map has no field name to scan, so a secret that enters one is caught where
/// it is copied in: an `.insert(...)` statement whose arguments `.expose()` a `SecretString`
/// turns it back into a plain string inside a collection. Store the `SecretString` itself,
/// or carry the secret in a typed field. Reports as struct `<insert>`, field `expose`.
fn scan_exposed_inserts(file: &str, source: &str) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || !trimmed.contains(".insert(") {
            continue;
        }
        let mut statement = String::new();
        for next in lines.iter().skip(index).take(8) {
            statement.push_str(next.trim());
            if statement.ends_with(';') || statement.ends_with("});") {
                break;
            }
        }
        let arguments = statement
            .split_once(".insert(")
            .map_or("", |(_, rest)| rest);
        if arguments.contains(".expose()") {
            findings.push(Finding {
                file: file.to_owned(),
                line: index + 1,
                strukt: "<insert>".to_owned(),
                field: "expose".to_owned(),
                ty: statement,
            });
        }
    }
    findings
}

/// Every plain-string header-collection field of a `derive(Debug)` struct in `source`.
fn scan_headers(file: &str, source: &str) -> Vec<Finding> {
    scan_fields(file, source, derives_debug, plain_header_collection)
}

/// Whether the attribute text in front of a struct derives `Debug`.
fn derives_debug(attributes: &str) -> bool {
    attributes
        .split("#[derive(")
        .skip(1)
        .any(|derive| derive.split([',', ')']).any(|name| name.trim() == "Debug"))
}

/// Every field that `flagged(name, type)` matches, in each struct of `source` whose joined
/// attribute text (derives and other attributes) `selected` accepts.
fn scan_fields(
    file: &str,
    source: &str,
    selected: impl Fn(&str) -> bool,
    flagged: impl Fn(&str, &str) -> bool,
) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();
    let mut attributes = String::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("#[") {
            let mut attribute = trimmed.to_owned();
            while !attribute.ends_with(']') && i + 1 < lines.len() {
                i += 1;
                attribute.push_str(lines[i].trim());
            }
            attributes.push_str(&attribute);
            i += 1;
            continue;
        }
        if trimmed.starts_with("//") {
            i += 1;
            continue;
        }
        let item_attributes = std::mem::take(&mut attributes);
        let item = strip_visibility(trimmed);
        i += 1;
        let Some(rest) = item.strip_prefix("struct ") else {
            continue;
        };
        if !item.ends_with('{') || !selected(&item_attributes) {
            continue; // unit or tuple struct (no named fields), or not selected
        }
        let strukt = identifier(rest).to_owned();
        while i < lines.len() && lines[i].trim() != "}" {
            let line = strip_visibility(lines[i].trim());
            if let Some((name, ty)) = line.split_once(':') {
                let name = name.trim();
                let ty = ty.trim().trim_end_matches(',');
                if identifier(name) == name && flagged(name, ty) {
                    findings.push(Finding {
                        file: file.to_owned(),
                        line: i + 1,
                        strukt: strukt.clone(),
                        field: name.to_owned(),
                        ty: ty.to_owned(),
                    });
                }
            }
            i += 1;
        }
    }
    findings
}

/// Run `scanner` over every crate source except this file (its fixtures are leaky).
fn scan_workspace(scanner: fn(&str, &str) -> Vec<Finding>) -> Vec<Finding> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    assert!(files.len() > 50, "scanned too few sources: {}", files.len());

    let mut findings = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == SELF {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        findings.extend(scanner(&rel, &source));
    }
    findings
}

#[test]
fn hs2_cqne4e_no_derived_debug_over_plain_string_headers() {
    let offenders: Vec<String> = scan_workspace(scan_headers)
        .iter()
        .map(|f| format!("{}:{} {}.{}: {}", f.file, f.line, f.strukt, f.field, f.ty))
        .collect();
    assert!(
        offenders.is_empty(),
        "derive(Debug) over a plain-string header collection; write Debug by hand with \
         hotsheet_sync::secret::RedactedHeaders:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn hs2_cqne4e_header_scanner_flags_plain_collections_only() {
    let source = r#"
#[derive(Debug, Clone)]
pub struct Leaky {
    pub headers: Vec<(String, String)>,
    pub(crate) response_headers: HashMap<String, String>,
    extra_headers: BTreeMap<&'static str, String>,
    raw_headers: HeaderMap,
    pub safe_headers: Vec<(String, SecretString)>,
    pub header_count: usize,
    pub url: String,
}

#[derive(Clone)]
struct HandWritten {
    headers: Vec<(String, String)>,
}
"#;
    let found: Vec<String> = scan_headers("x.rs", source)
        .into_iter()
        .map(|f| f.field)
        .collect();
    assert_eq!(
        found,
        [
            "headers",
            "response_headers",
            "extra_headers",
            "raw_headers"
        ]
    );
}

#[test]
fn hs2_n1jxbz_no_struct_holds_a_plain_string_secret() {
    let findings = scan_workspace(scan);

    let exempt = |f: &Finding| {
        NOT_SECRET
            .iter()
            .any(|(file, s, field, _)| f.file == *file && f.strukt == *s && f.field == *field)
    };
    let offenders: Vec<String> = findings
        .iter()
        .filter(|f| !exempt(f))
        .map(|f| format!("{}:{} {}.{}: {}", f.file, f.line, f.strukt, f.field, f.ty))
        .collect();
    assert!(
        offenders.is_empty(),
        "a struct holds a plain-string secret; use hotsheet_sync::secret::SecretString and \
         .expose() it at the transport (serde and clap formats are unchanged):\n{}",
        offenders.join("\n")
    );

    // Every exemption must still match, so a fixed or renamed field drops out of the list.
    let stale: Vec<String> = NOT_SECRET
        .iter()
        .map(|(file, s, field, _)| (*file, *s, *field))
        .filter(|(file, s, field)| {
            !findings
                .iter()
                .any(|f| f.file == *file && f.strukt == *s && f.field == *field)
        })
        .map(|(file, s, field)| format!("{file} {s}.{field}"))
        .collect();
    assert!(
        stale.is_empty(),
        "stale HS2-A8T481 exemptions:\n{}",
        stale.join("\n")
    );
}

#[test]
fn hs2_a8t481_scanner_flags_plain_secrets_and_accepts_the_redacting_type() {
    let source = r#"
#[derive(Clone, Debug, Serialize)]
/// Doc.
#[serde(rename_all = "snake_case")]
pub struct Leaky {
    pub token: String,
    pub(crate) refresh_token: Option<String>,
    api_key: String,
    pub token_type: Option<String>,
    pub password: SecretString,
    pub secret: Option<SecretString>,
    pub fencing_token: u64,
}

#[derive(Debug)]
struct Session {
    csrf: String,
    session_cookie: Option<String>,
    authorization: Option<String>,
    client_private_key: String,
    csrf_hash: String,
    session_csrf: Option<SecretString>,
}

#[derive(
    Clone,
    Debug,
)]
struct MultiLine {
    secret: String,
}

#[derive(Clone)]
struct NoDebug {
    password: String,
}

#[derive(Debug)]
struct Tuple(String);
"#;
    let found: Vec<(String, String)> = scan("x.rs", source)
        .into_iter()
        .map(|f| (f.strukt, f.field))
        .collect();
    let expected = [
        ("Leaky", "token"),
        ("Leaky", "refresh_token"),
        ("Leaky", "api_key"),
        ("Session", "csrf"),
        ("Session", "session_cookie"),
        ("Session", "authorization"),
        ("Session", "client_private_key"),
        ("MultiLine", "secret"),
        ("NoDebug", "password"),
    ]
    .map(|(s, f)| (s.to_owned(), f.to_owned()));
    assert_eq!(found, expected);
}

#[test]
fn hs2_n1jxbz_scanner_flags_secrets_whatever_the_struct_derives() {
    let source = r#"
pub struct AppState {
    pub secret: String,
    pub port: u16,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    /// The bearer.
    #[serde(default)]
    pub bearer_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_csrf: Option<String>,
    pub safe_secret: SecretString,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials(<redacted>)")
    }
}

#[derive(Parser)]
struct Cli {
    /// Machine shared secret.
    #[arg(long, env = "HOTSHEET_SECRET")]
    secret: Option<String>,
    #[arg(long)]
    other_secret: Option<SecretString>,
}

fn helper() {
    struct Local {
        token: String,
    }
}

#[derive(Debug)]
struct Tuple(String);
"#;
    let found: Vec<(String, String)> = scan("x.rs", source)
        .into_iter()
        .map(|f| (f.strukt, f.field))
        .collect();
    let expected = [
        ("AppState", "secret"),
        ("Credentials", "bearer_secret"),
        ("Credentials", "session_csrf"),
        ("Cli", "secret"),
        ("Local", "token"),
    ]
    .map(|(s, f)| (s.to_owned(), f.to_owned()));
    assert_eq!(found, expected);

    // The header rule stays scoped to a derived `Debug` (HS2-CQNE4E).
    assert!(derives_debug("#[derive(Clone,Debug,)]#[serde(default)]"));
    assert!(!derives_debug(
        "#[derive(Clone, Serialize)]#[serde(rename = \"Debug\")]"
    ));
}

#[test]
fn hs2_7aj57b_key_pem_names_match_and_public_pems_do_not() {
    for name in [
        "key_pem",
        "client_key_pem",
        "private_key_pem",
        "device_key_pem",
    ] {
        assert!(secret_named(name), "{name}");
    }
    for name in [
        "cert_pem",
        "ca_pem",
        "client_certificate_pem",
        "key",
        "map_key",
        "key_pem_path",
        "keypair",
        "api_key_id",
    ] {
        assert!(!secret_named(name), "{name}");
    }
    assert_eq!(snake_case("ApiToken"), "api_token");
    assert_eq!(snake_case("Bearer"), "bearer");
    assert_eq!(snake_case("ClientKeyPem"), "client_key_pem");
}

#[test]
fn hs2_7aj57b_scanner_flags_enum_variants_and_tuple_structs() {
    let source = r#"
#[derive(Debug, Clone)]
pub enum Auth {
    /// Doc.
    Basic {
        user: String,
        password: String,
        #[serde(default)]
        pub(crate) session_csrf: Option<SecretString>,
    },
    Device { client_key_pem: Option<String>, name: String },
    Bearer(String),
    ApiToken(Option<&'static str>),
    Secret(SecretString),
    Invalid(String),
    None,
}

pub struct ApiToken(pub String);
struct SessionSecret(SecretString);
#[derive(Debug)]
struct Tuple(String);
struct Unit;
pub(crate) enum Empty {}
"#;
    let found: Vec<(String, String)> = scan("x.rs", source)
        .into_iter()
        .map(|f| (f.strukt, f.field))
        .collect();
    let expected = [
        ("Auth::Basic", "password"),
        ("Auth::Device", "client_key_pem"),
        ("Auth::Bearer", "0"),
        ("Auth::ApiToken", "0"),
        ("ApiToken", "0"),
    ]
    .map(|(s, f)| (s.to_owned(), f.to_owned()));
    assert_eq!(found, expected);
}

#[test]
fn hs2_7aj57b_scanner_flags_an_exposed_secret_copied_into_a_collection() {
    let source = r#"
fn build(token: &SecretString, map: &mut BTreeMap<String, String>) {
    map.insert(TOKEN_FIELD.to_string(), token.expose().to_owned());
    map.insert(
        "secret".into(),
        host.secret.expose().to_owned(),
    );
    // map.insert(k, token.expose().to_owned());
    map.insert("socket".into(), socket.clone());
    secrets.insert(name, token.clone());
    let value = token.expose().to_owned();
}
"#;
    let found: Vec<usize> = scan("x.rs", source)
        .into_iter()
        .inspect(|f| {
            assert_eq!(
                (f.strukt.as_str(), f.field.as_str()),
                ("<insert>", "expose")
            )
        })
        .map(|f| f.line)
        .collect();
    assert_eq!(found, [3, 4]);
}
