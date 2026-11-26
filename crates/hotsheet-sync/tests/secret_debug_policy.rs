//! HS2-A8T481 lint: a struct that derives `Debug` may not hold a secret-named string field
//! (one of `SECRET_WORDS`, such as `token`, `secret`, `csrf` or `authorization`, or a
//! `_`-suffixed form such as `access_token`) as a plain `String`. Such a field uses `hotsheet_sync::secret::SecretString`, whose
//! `Debug` prints `<redacted>`. Otherwise the next `dbg!`, `{:?}` in an error, or failing
//! `assert_eq!` prints the credential. A hand-written `Debug` that redacts is also allowed,
//! since this lint only inspects derived `Debug`.
//!
//! HS2-CQNE4E extends it to header collections: a `derive(Debug)` struct may not hold a
//! `*header*` field typed as plain strings (`Vec<(String, String)>`, a `String` map, or
//! `HeaderMap`), because `Authorization`, cookie and signature values print with it. Such a
//! struct writes its `Debug` by hand with `hotsheet_sync::secret::RedactedHeaders`, or holds
//! the values as `SecretString`.

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

/// Every secret-named plain-string field of a `derive(Debug)` struct in `source`.
fn scan(file: &str, source: &str) -> Vec<Finding> {
    scan_fields(file, source, |name, ty| {
        secret_named(name) && plain_string(ty)
    })
}

/// Every plain-string header-collection field of a `derive(Debug)` struct in `source`.
fn scan_headers(file: &str, source: &str) -> Vec<Finding> {
    scan_fields(file, source, plain_header_collection)
}

/// Every field of a `derive(Debug)` struct in `source` that `flagged(name, type)` matches.
fn scan_fields(file: &str, source: &str, flagged: impl Fn(&str, &str) -> bool) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if !trimmed.starts_with("#[derive(") {
            i += 1;
            continue;
        }
        let mut derive = trimmed.to_owned();
        while !derive.contains(")]") && i + 1 < lines.len() {
            i += 1;
            derive.push_str(lines[i].trim());
        }
        i += 1;
        let derives_debug = derive
            .trim_start_matches("#[derive(")
            .split([',', ')'])
            .any(|name| name.trim() == "Debug");
        if !derives_debug {
            continue;
        }
        // Skip further attributes and doc comments between the derive and the item.
        while i < lines.len() {
            let next = lines[i].trim();
            if next.starts_with("#[") || next.starts_with("//") {
                i += 1;
            } else {
                break;
            }
        }
        let Some(item) = lines.get(i).map(|line| strip_visibility(line.trim())) else {
            break;
        };
        let Some(rest) = item.strip_prefix("struct ") else {
            continue;
        };
        if !item.ends_with('{') {
            continue; // unit or tuple struct: no named fields
        }
        let strukt = identifier(rest).to_owned();
        i += 1;
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
fn hs2_a8t481_no_derived_debug_over_a_plain_string_secret() {
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
        "derive(Debug) over a plain-string secret; use hotsheet_sync::secret::SecretString \
         (or a hand-written redacting Debug):\n{}",
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
    ]
    .map(|(s, f)| (s.to_owned(), f.to_owned()));
    assert_eq!(found, expected);
}
