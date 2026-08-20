//! `hotsheet-migrate` — the standalone, disposable Hot Sheet 1 → Hot Sheet 2 migrator
//! (`docs/07-migration.md`). Kept OUT of the main `hotsheet` CLI on purpose: it's a
//! rarely-used, one-time tool that needs Node + the bundled exporter, unlike the
//! always-on ticket commands. It opens only a COPY of the old database (read-only).

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "hotsheet-migrate",
    version,
    about = "Migrate a Hot Sheet 1 project into a Hot Sheet 2 store"
)]
struct Cli {
    /// The old project's `.hotsheet` directory.
    hotsheet_dir: PathBuf,
    /// Destination store directory (created if it isn't one yet).
    #[arg(short = 'C', long = "path", default_value = ".")]
    path: PathBuf,
    /// Prefix used if the store must be created.
    #[arg(long, default_value = "HS")]
    prefix: String,
    /// Path to the migrator's `export.mjs` (auto-detected, or $HOTSHEET_MIGRATOR).
    #[arg(long)]
    migrator: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let summary =
        hotsheet_cli::run_migrate(&cli.path, &cli.hotsheet_dir, &cli.prefix, cli.migrator)?;
    println!(
        "Imported {} ticket(s) ({} attachment file(s)), skipped {} already present.",
        summary.written, summary.attachments, summary.skipped
    );
    Ok(())
}
