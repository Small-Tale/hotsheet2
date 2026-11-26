//! HS2-A8T481 lint: a struct that derives `Debug` may not hold a secret-named string field
//! (`token`, `secret`, `password`, `api_key`, or a `_`-suffixed form such as `access_token`)
//! as a plain `String`. Such a field uses `hotsheet_sync::secret::SecretString`, whose
//! `Debug` prints `<redacted>`. Otherwise the next `dbg!`, `{:?}` in an error, or failing
//! `assert_eq!` prints the credential. A hand-written `Debug` that redacts is also allowed,
//! since this lint only inspects derived `Debug`.

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
];

/// Real secrets found when this lint landed and not yet retyped. HS2-9ZMC7N owns them.
/// This list only shrinks: retype a field to `SecretString` and delete its entry.
const BASELINE: &[(&str, &str, &str)] = &[
    ("hotsheet-server/src/lifecycle.rs", "InstanceInfo", "secret"),
    (
        "hotsheet-server/src/remote_auth.rs",
        "IssuedSession",
        "token",
    ),
    ("hotsheet-server/src/dto.rs", "WsParams", "secret"),
    (
        "hotsheet-server/src/routes/store_scoped.rs",
        "PollParams",
        "secret",
    ),
    (
        "hotsheet-cli/src/external_launch.rs",
        "ServerInstance",
        "secret",
    ),
    (
        "hotsheet-terminals/src/broker.rs",
        "BrokerPushState",
        "password",
    ),
    (
        "hotsheet-resource-claims/src/model.rs",
        "Grant",
        "renewal_token",
    ),
    (
        "hotsheet-extsync/src/gitlab/attachments.rs",
        "GitLabAttachmentMarker",
        "secret",
    ),
    (
        "hotsheet-extsync/src/gitlab/attachments.rs",
        "GitLabAttachmentMarker",
        "rendition_secret",
    ),
];

const SELF: &str = "hotsheet-sync/tests/secret_debug_policy.rs";

const SECRET_WORDS: &[&str] = &["token", "secret", "password", "api_key"];

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
                if identifier(name) == name && secret_named(name) && plain_string(ty) {
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

#[test]
fn hs2_a8t481_no_derived_debug_over_a_plain_string_secret() {
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
            continue; // its scanner fixture is deliberately leaky
        }
        let source = std::fs::read_to_string(&path).unwrap();
        findings.extend(scan(&rel, &source));
    }

    let exempt = |f: &Finding| {
        NOT_SECRET
            .iter()
            .any(|(file, s, field, _)| f.file == *file && f.strukt == *s && f.field == *field)
            || BASELINE
                .iter()
                .any(|(file, s, field)| f.file == *file && f.strukt == *s && f.field == *field)
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

    // Every exemption must still match, so a fixed or renamed field drops out of the lists.
    let stale: Vec<String> = NOT_SECRET
        .iter()
        .map(|(file, s, field, _)| (*file, *s, *field))
        .chain(BASELINE.iter().copied())
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
    assert!(BASELINE.len() <= 9, "the HS2-A8T481 baseline only shrinks");
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
        ("MultiLine", "secret"),
    ]
    .map(|(s, f)| (s.to_owned(), f.to_owned()));
    assert_eq!(found, expected);
}
