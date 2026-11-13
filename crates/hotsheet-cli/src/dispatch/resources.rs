//! `hotsheet-cli resource`: headless resource claims (HS2-FMHH9Y,
//! `docs/36-resource-claims.md` §36.10). Every subcommand answers through the same
//! `hotsheet_resource_claims::ClaimService` request workflow the server routes and the
//! MCP tool use, against the machine home, and needs no running server.

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use hotsheet_resource_claims::{
    ClaimError, ClaimService, Namespace, ROUTE_PREFIX, ResourceRef, TOKEN_ROUTE_PREFIX,
    parse_duration,
};
use serde_json::{Map, Value, json};

use super::{Ctx, misrouted};
use crate::Cmd;

/// Environment variable a task reads its renewal token from when `--token` is absent,
/// so the token need not appear on a command line.
const TOKEN_ENV: &str = "HOTSHEET_RESOURCE_CLAIM_TOKEN";

#[derive(Subcommand)]
pub(crate) enum ResourceCmd {
    /// Claim `<namespace>/<resource>`. Prints the claim id, fencing token, renewal token
    /// and expiry as JSON. Fails at once when it is held unless --wait is given.
    Acquire {
        /// `<namespace>/<resource>`.
        target: String,
        /// How long the lease lasts unless renewed: `90s`, `30m`, `1h30m` (at most 24h).
        #[arg(long)]
        duration: String,
        /// Wait this long for a release or an expiry instead of failing at once.
        #[arg(long)]
        wait: Option<String>,
        /// Holder metadata `key=value` (repeatable). A `plain:`, `hmac-sha256:` or
        /// `enc:v1:` prefix on the value selects its codec; no prefix stores `plain:`.
        #[arg(long = "meta", value_name = "KEY=VALUE")]
        meta: Vec<String>,
    },
    /// Extend a claim with its renewal token alone.
    Renew {
        /// Optional `<namespace>/<resource>` the token must be for.
        target: Option<String>,
        /// The renewal token (default: `$HOTSHEET_RESOURCE_CLAIM_TOKEN`).
        #[arg(long)]
        token: Option<String>,
        /// The new lease length from now.
        #[arg(long)]
        duration: String,
    },
    /// End a claim early with its renewal token alone; the token stops working.
    Release {
        /// Optional `<namespace>/<resource>` the token must be for.
        target: Option<String>,
        /// The renewal token (default: `$HOTSHEET_RESOURCE_CLAIM_TOKEN`).
        #[arg(long)]
        token: Option<String>,
    },
    /// Show whether `<namespace>/<resource>` is held, and by which claim.
    Show {
        /// `<namespace>/<resource>`.
        target: String,
        /// Wait this long for it to become free, without claiming it.
        #[arg(long)]
        wait: Option<String>,
    },
    /// List the live claims in a namespace.
    List { namespace: String },
}

pub(crate) fn run(cmd: Cmd, _ctx: &Ctx) -> Result<()> {
    let Cmd::Resource { cmd } = cmd else {
        misrouted("resources")
    };
    let service = ClaimService::new(hotsheet_plugins::hotsheet_home());
    let (method, path, body) = request(cmd)?;
    match service.request(method, &path, &body) {
        Ok(answer) => {
            print(&answer)?;
            // A refused acquire is a normal answer for a script to read, and a failed
            // command for a shell to branch on.
            if answer.get("acquired") == Some(&Value::Bool(false)) {
                bail!("{}", answer["error"].as_str().unwrap_or("resource is held"));
            }
            Ok(())
        }
        Err(error) => {
            print(&json!({ "error": error.to_string(), "code": error.code() }))?;
            Err(anyhow!("{}: {error}", error.code()))
        }
    }
}

fn print(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Translate a subcommand into the shared request shape.
fn request(cmd: ResourceCmd) -> Result<(&'static str, String, Value)> {
    Ok(match cmd {
        ResourceCmd::Acquire {
            target,
            duration,
            wait,
            meta,
        } => {
            let mut body = json!({
                "duration_ms": millis(&duration)?,
                "metadata": metadata(&meta)?,
            });
            if let Some(wait) = wait {
                body["wait_ms"] = millis(&wait)?.into();
            }
            ("POST", target_path(&target, Some("acquire"))?, body)
        }
        ResourceCmd::Renew {
            target,
            token,
            duration,
        } => {
            let body =
                json!({ "renewal_token": token_value(token)?, "duration_ms": millis(&duration)? });
            ("POST", token_path(target.as_deref(), "renew")?, body)
        }
        ResourceCmd::Release { target, token } => {
            let body = json!({ "renewal_token": token_value(token)? });
            ("POST", token_path(target.as_deref(), "release")?, body)
        }
        ResourceCmd::Show { target, wait: None } => {
            ("GET", target_path(&target, None)?, Value::Null)
        }
        ResourceCmd::Show {
            target,
            wait: Some(wait),
        } => (
            "POST",
            target_path(&target, Some("wait"))?,
            json!({ "wait_ms": millis(&wait)? }),
        ),
        ResourceCmd::List { namespace } => {
            let namespace = Namespace::parse(&namespace).map_err(cli_error)?;
            ("GET", format!("{ROUTE_PREFIX}/{namespace}"), Value::Null)
        }
    })
}

fn cli_error(error: ClaimError) -> anyhow::Error {
    anyhow!("{}: {error}", error.code())
}

fn millis(text: &str) -> Result<u64> {
    let duration = parse_duration(text).map_err(cli_error)?;
    u64::try_from(duration.as_millis()).context("duration is too long")
}

fn target_path(target: &str, action: Option<&str>) -> Result<String> {
    let target = ResourceRef::parse(target).map_err(cli_error)?;
    Ok(match action {
        Some(action) => format!("{ROUTE_PREFIX}/{target}/{action}"),
        None => format!("{ROUTE_PREFIX}/{target}"),
    })
}

/// With a target the token must be for that resource; without one the token alone
/// names its claim.
fn token_path(target: Option<&str>, action: &str) -> Result<String> {
    match target {
        Some(target) => target_path(target, Some(action)),
        None => Ok(format!("{TOKEN_ROUTE_PREFIX}/{action}")),
    }
}

fn token_value(flag: Option<String>) -> Result<String> {
    flag.or_else(|| std::env::var(TOKEN_ENV).ok())
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| anyhow!("pass --token or set {TOKEN_ENV}"))
}

fn metadata(entries: &[String]) -> Result<Map<String, Value>> {
    let mut map = Map::new();
    for entry in entries {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| anyhow!("--meta takes key=value"))?;
        if map.insert(key.to_owned(), value.into()).is_some() {
            bail!("--meta names '{key}' more than once");
        }
    }
    Ok(map)
}
