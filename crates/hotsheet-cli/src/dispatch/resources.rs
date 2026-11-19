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
        #[command(flatten)]
        scope: super::resource_backends::ScopeArg,
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
        /// Renew through this Hot Sheet server's token route instead of the machine home:
        /// `https://<host>[:<port>]`. Only the token is sent. For a task with no shared home.
        #[arg(long, value_name = "URL")]
        server: Option<String>,
        /// With --server: trust exactly this CA certificate (PEM), such as the one
        /// `hotsheet-cli cert init` writes, instead of the public web PKI.
        #[arg(long, value_name = "FILE", requires = "server")]
        ca_cert: Option<std::path::PathBuf>,
        /// With --server: allow plain http:// to a non-loopback address (a host-only network
        /// no other machine can reach); the token crosses in clear text.
        #[arg(long, requires = "server")]
        allow_insecure_http: bool,
        #[command(flatten)]
        scope: super::resource_backends::ScopeArg,
    },
    /// End a claim early with its renewal token alone; the token stops working.
    Release {
        /// Optional `<namespace>/<resource>` the token must be for.
        target: Option<String>,
        /// The renewal token (default: `$HOTSHEET_RESOURCE_CLAIM_TOKEN`).
        #[arg(long)]
        token: Option<String>,
        /// Release through this Hot Sheet server's token route; see `renew --server`.
        #[arg(long, value_name = "URL")]
        server: Option<String>,
        /// With --server: trust exactly this CA certificate (PEM).
        #[arg(long, value_name = "FILE", requires = "server")]
        ca_cert: Option<std::path::PathBuf>,
        /// With --server: allow plain http:// to a non-loopback address.
        #[arg(long, requires = "server")]
        allow_insecure_http: bool,
        #[command(flatten)]
        scope: super::resource_backends::ScopeArg,
    },
    /// Show whether `<namespace>/<resource>` is held, and by which claim.
    Show {
        /// `<namespace>/<resource>`.
        target: String,
        /// Wait this long for it to become free, without claiming it.
        #[arg(long)]
        wait: Option<String>,
        #[command(flatten)]
        scope: super::resource_backends::ScopeArg,
    },
    /// List the live claims in a namespace.
    List {
        namespace: String,
        #[command(flatten)]
        scope: super::resource_backends::ScopeArg,
    },
    /// Claim backend configurations: add, list, show, test, remove, key, secret (HS2-JJWESD, HS2-322ZSZ).
    Backend {
        #[command(subcommand)]
        cmd: super::resource_backends::BackendCmd,
    },
    /// External claim-backend plugins: list them, approve one (HS2-322ZSZ).
    Plugin {
        #[command(subcommand)]
        cmd: super::resource_backends::PluginCmd,
    },
    /// Bind a resource namespace to a configuration, or unbind it.
    Namespace {
        #[command(subcommand)]
        cmd: super::resource_backends::NamespaceCmd,
    },
    /// Route a ticket data source's claims through a configuration, or stop.
    Source {
        #[command(subcommand)]
        cmd: super::resource_backends::SourceCmd,
    },
}

pub(crate) fn run(cmd: Cmd, _ctx: &Ctx) -> Result<()> {
    let Cmd::Resource { cmd } = cmd else {
        misrouted("resources")
    };
    let cmd = match cmd {
        // HS2-7H1Y4E: through a server's token route, with the token alone.
        ResourceCmd::Renew {
            target,
            token,
            duration,
            server: Some(server),
            ca_cert,
            allow_insecure_http,
            ..
        } => {
            return super::resource_remote::call(
                &server,
                ca_cert.as_deref(),
                allow_insecure_http,
                "renew",
                target.as_deref(),
                &token_value(token)?,
                Some(millis(&duration)?),
            );
        }
        ResourceCmd::Release {
            target,
            token,
            server: Some(server),
            ca_cert,
            allow_insecure_http,
            ..
        } => {
            return super::resource_remote::call(
                &server,
                ca_cert.as_deref(),
                allow_insecure_http,
                "release",
                target.as_deref(),
                &token_value(token)?,
                None,
            );
        }
        ResourceCmd::Backend { cmd } => return super::resource_backends::backend(cmd),
        ResourceCmd::Plugin { cmd } => return super::resource_backends::plugin(cmd),
        ResourceCmd::Namespace { cmd } => return super::resource_backends::namespace(cmd),
        ResourceCmd::Source { cmd } => return super::resource_backends::source(cmd),
        other => other,
    };
    let mut service = ClaimService::new(hotsheet_plugins::hotsheet_home());
    if let Some(project) = cmd_project(&cmd) {
        service = service.with_project(project);
    }
    let (method, path, body) = request(cmd)?;
    // HS2-QGY1NN: a wait is ended by SIGTERM or SIGINT cleanly, so its waiter registration is
    // removed instead of counting against the namespace until the wait's deadline.
    let waits =
        path.ends_with("/wait") || (path.ends_with("/acquire") && body.get("wait_ms").is_some());
    let _interrupts = if waits {
        Some(hotsheet_resource_claims::InterruptGuard::new()?)
    } else {
        None
    };
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

/// The project whose registry a claim command also consults.
fn cmd_project(cmd: &ResourceCmd) -> Option<std::path::PathBuf> {
    match cmd {
        ResourceCmd::Acquire { scope, .. }
        | ResourceCmd::Renew { scope, .. }
        | ResourceCmd::Release { scope, .. }
        | ResourceCmd::Show { scope, .. }
        | ResourceCmd::List { scope, .. } => scope.project(),
        _ => None,
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
            ..
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
            ..
        } => {
            let body =
                json!({ "renewal_token": token_value(token)?, "duration_ms": millis(&duration)? });
            ("POST", token_path(target.as_deref(), "renew")?, body)
        }
        ResourceCmd::Release { target, token, .. } => {
            let body = json!({ "renewal_token": token_value(token)? });
            ("POST", token_path(target.as_deref(), "release")?, body)
        }
        ResourceCmd::Show {
            target, wait: None, ..
        } => ("GET", target_path(&target, None)?, Value::Null),
        ResourceCmd::Show {
            target,
            wait: Some(wait),
            ..
        } => (
            "POST",
            target_path(&target, Some("wait"))?,
            json!({ "wait_ms": millis(&wait)? }),
        ),
        ResourceCmd::List { namespace, .. } => {
            let namespace = Namespace::parse(&namespace).map_err(cli_error)?;
            ("GET", format!("{ROUTE_PREFIX}/{namespace}"), Value::Null)
        }
        ResourceCmd::Backend { .. }
        | ResourceCmd::Plugin { .. }
        | ResourceCmd::Namespace { .. }
        | ResourceCmd::Source { .. } => {
            bail!("not a claim request")
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
