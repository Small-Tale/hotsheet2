//! Shared logic for the Hot Sheet CLI binaries.
//!
//! Holds the HS1 **import** path (the pglite-free half of migration — it only reads a
//! portable JSON) plus the store's git helpers, so both binaries can reuse it with no
//! format drift:
//! - `hotsheet` (the main CLI) uses `run_import` for its `import` command.
//! - `hotsheet-migrate` (the standalone, disposable migrator) uses `run_migrate`,
//!   which spawns the Node exporter and then imports.
//!
//! Migration lives in its own binary on purpose: it's rarely used, one-time, and
//! needs Node + the bundled exporter — none of which the always-on ticket commands
//! should carry (`docs/07-migration.md` §7.2).

pub mod import;
// Launch-safety machinery lives in the shared `hotsheet-aitools` crate (so the server can
// reuse it too, HS2-1TY7GC); re-exported here to keep the `hotsheet_cli::launch_safety` path.
pub use hotsheet_aitools::launch_safety;
pub mod plugin;
pub mod setup;
pub mod workloop;

pub use setup::{SetupReport, run_setup};

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use hotsheet_ticketing::{FsStore, StoreMetadata};

use crate::import::{ExportFile, ImportSummary, SUPPORTED_EXPORT_VERSION, import};

/// Read a `hotsheet-export.json` and import it into the store (creating the store on
/// first import). Prints progress + warnings and commits; returns the summary so the
/// caller can phrase the final line.
pub fn run_import(store_path: &Path, export_file: &Path, prefix: &str) -> Result<ImportSummary> {
    let text = std::fs::read_to_string(export_file)
        .with_context(|| format!("reading export {}", export_file.display()))?;
    let export: ExportFile = serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", export_file.display()))?;

    if export.export_version != SUPPORTED_EXPORT_VERSION {
        eprintln!(
            "warning: export version {} differs from supported {SUPPORTED_EXPORT_VERSION}; \
             importing on a best-effort basis",
            export.export_version
        );
    }
    if let Some(name) = &export.project.name {
        println!("Importing project '{name}'…");
    }

    // Create the store on first import, preferring the export's own prefix.
    let store = match FsStore::open(store_path) {
        Ok(store) => store,
        Err(_) => {
            let init_prefix = export.project.ticket_prefix.as_deref().unwrap_or(prefix);
            let store = FsStore::init(store_path, &StoreMetadata::new(init_prefix))?;
            git_init(store_path);
            register_merge_driver(store_path);
            store
        }
    };

    let base_dir = export_file.parent().unwrap_or_else(|| Path::new("."));
    let summary = import(&store, &export, base_dir)?;
    if summary.written > 0 {
        git_commit_all(
            store_path,
            &format!("Import {} tickets from Hot Sheet 1", summary.written),
        );
    }
    Ok(summary)
}

/// Migrate a Hot Sheet 1 project in one step: run the Node exporter against a COPY of
/// `hotsheet_dir`'s database into a temp export, then import it.
pub fn run_migrate(
    store_path: &Path,
    hotsheet_dir: &Path,
    prefix: &str,
    migrator: Option<PathBuf>,
) -> Result<ImportSummary> {
    let export_mjs = resolve_migrator(migrator)?;

    // A private temp dir for the export JSON + staged attachments. The exporter only
    // ever opens a COPY of the source database (read-only).
    let staging = std::env::temp_dir().join(format!("hotsheet-migrate-{}", std::process::id()));
    std::fs::create_dir_all(&staging)?;
    let export_json = staging.join("hotsheet-export.json");

    println!("Exporting {} …", hotsheet_dir.display());
    let status = Command::new("node")
        .arg(&export_mjs)
        .arg(hotsheet_dir)
        .arg("--out")
        .arg(&export_json)
        .status()
        .with_context(|| {
            format!(
                "running the migrator ({}) — is Node installed?",
                export_mjs.display()
            )
        })?;
    if !status.success() {
        bail!("migrator export failed ({status})");
    }

    let result = run_import(store_path, &export_json, prefix);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Find the Node exporter: an explicit path, `$HOTSHEET_MIGRATOR`, or a few locations
/// relative to the CWD / executable.
pub fn resolve_migrator(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        if p.is_file() {
            return Ok(p);
        }
        bail!("migrator not found at {}", p.display());
    }
    if let Ok(env) = std::env::var("HOTSHEET_MIGRATOR") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Ok(p);
        }
    }
    let mut candidates = vec![PathBuf::from("migrator/src/export.mjs")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../../migrator/src/export.mjs"));
            candidates.push(dir.join("../../../migrator/src/export.mjs"));
        }
    }
    candidates
        .into_iter()
        .find(|c| c.is_file())
        .with_context(|| {
            "could not find the migrator (migrator/src/export.mjs); \
         pass --migrator <path> or set HOTSHEET_MIGRATOR"
                .to_string()
        })
}

/// Best-effort `git init` of a new store (warns, never fails the command).
pub fn git_init(path: &Path) {
    if path.join(".git").exists() {
        return;
    }
    run_git(path, &["init", "--quiet"]);
}

/// The `.gitattributes` line + git config value that register the semantic merge driver.
const MERGE_ATTR_LINE: &str = "tickets/**/*.md merge=hotsheet-ticket";
const MERGE_DRIVER_NAME: &str = "hotsheet-ticket";

/// Register the semantic merge driver in a store (HS2-18, docs/02 §2.7): write the
/// `.gitattributes` line so git routes `tickets/**/*.md` through `merge=hotsheet-ticket`,
/// and point that driver at this binary's `merge-driver` subcommand via git config.
/// Best-effort + idempotent — safe to re-run (e.g. from `init` or a repair).
pub fn register_merge_driver(store_path: &Path) {
    // .gitattributes — append the line once, preserving any existing rules.
    let ga = store_path.join(".gitattributes");
    let existing = std::fs::read_to_string(&ga).unwrap_or_default();
    if !existing.lines().any(|l| l.trim() == MERGE_ATTR_LINE) {
        let mut content = existing;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("# Hot Sheet semantic ticket merge (HS2-18)\n");
        content.push_str(MERGE_ATTR_LINE);
        content.push('\n');
        if let Err(e) = std::fs::write(&ga, content) {
            eprintln!("warning: could not write {}: {e}", ga.display());
        }
    }
    // git config — the driver command git runs (%O base, %A ours/output, %B theirs).
    let driver = match std::env::current_exe() {
        Ok(exe) => format!("\"{}\" merge-driver %O %A %B", exe.display()),
        Err(_) => "hotsheet-cli merge-driver %O %A %B".to_string(),
    };
    run_git(
        store_path,
        &[
            "config",
            &format!("merge.{MERGE_DRIVER_NAME}.name"),
            "Hot Sheet semantic ticket merge",
        ],
    );
    run_git(
        store_path,
        &[
            "config",
            &format!("merge.{MERGE_DRIVER_NAME}.driver"),
            &driver,
        ],
    );
}

/// Whether the semantic merge driver is registered: the `.gitattributes` line is present
/// **and** git config carries the driver command (`hotsheet doctor` checks this).
pub fn merge_driver_registered(store_path: &Path) -> bool {
    let attr_ok = std::fs::read_to_string(store_path.join(".gitattributes"))
        .map(|s| s.lines().any(|l| l.trim() == MERGE_ATTR_LINE))
        .unwrap_or(false);
    let config_ok = Command::new("git")
        .current_dir(store_path)
        .args(["config", &format!("merge.{MERGE_DRIVER_NAME}.driver")])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false);
    attr_ok && config_ok
}

/// Best-effort `git add -A && git commit` (warns on failure; files are already written).
pub fn git_commit_all(path: &Path, message: &str) {
    run_git(path, &["add", "-A"]);
    run_git(path, &["commit", "--quiet", "-m", message]);
}

fn run_git(path: &Path, args: &[&str]) {
    match Command::new("git").current_dir(path).args(args).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("warning: git {} exited with {status}", args.join(" ")),
        Err(err) => eprintln!("warning: could not run git {}: {err}", args.join(" ")),
    }
}
