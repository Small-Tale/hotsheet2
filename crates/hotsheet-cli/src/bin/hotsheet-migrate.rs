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
    #[arg(required_unless_present_any = ["verify_backup", "hold_job_lock"])]
    hotsheet_dir: Option<PathBuf>,
    /// Verify a manually pushed origin and record cleanup eligibility, without importing.
    #[arg(long, conflicts_with = "hotsheet_dir")]
    verify_backup: bool,
    /// Stream versioned NDJSON phase progress and the typed final result.
    #[arg(long, conflicts_with = "verify_backup")]
    progress_json: bool,
    /// Internal bridge guard: hold native locks until stdin closes.
    #[arg(long, hide = true, num_args = 1.., conflicts_with_all = ["hotsheet_dir", "verify_backup", "progress_json"])]
    hold_job_lock: Vec<PathBuf>,
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
    if !cli.hold_job_lock.is_empty() {
        use std::io::Write;
        let mut locks = Vec::new();
        for path in &cli.hold_job_lock {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path)?;
            try_job_lock(&file).map_err(|error| {
                anyhow::anyhow!("Migration repository is already owned: {error}")
            })?;
            locks.push(file);
        }
        println!("locked");
        std::io::stdout().flush()?;
        std::io::copy(&mut std::io::stdin(), &mut std::io::sink())?;
        drop(locks);
        return Ok(());
    }
    if cli.verify_backup {
        let revision = hotsheet_cli::verify_hs1_backup(&cli.path)?;
        println!("Verified Hot Sheet 1 backup for import {revision}.");
        return Ok(());
    }
    let source = cli
        .hotsheet_dir
        .expect("clap requires the HS1 source for migration");
    let result = hotsheet_cli::run_migrate_with_progress(
        &cli.path,
        &source,
        &cli.prefix,
        cli.migrator,
        &mut |event| {
            if cli.progress_json {
                use std::io::Write;
                println!(
                    "{}",
                    serde_json::to_string(&event).expect("serializable progress")
                );
                let _ = std::io::stdout().flush();
            }
        },
    );
    if cli.progress_json {
        match &result {
            Ok(summary) => println!(
                "{}",
                serde_json::json!({"version":1,"phase":"import_complete","result":{
                    "tickets": summary.written + summary.skipped,
                    "attachments": summary.attachments,
                    "written": summary.written,
                    "skipped": summary.skipped,
                }})
            ),
            Err(error) => println!(
                "{}",
                serde_json::json!({"version":1,"phase":"failed","error":format!("{error:#}")})
            ),
        }
        result?;
        return Ok(());
    }
    let summary = result?;
    println!(
        "Imported {} ticket(s) ({} attachment file(s)), skipped {} already present.",
        summary.written, summary.attachments, summary.skipped
    );
    Ok(())
}

#[cfg(unix)]
fn try_job_lock(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: flock borrows this live descriptor; closing it releases ownership.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
fn try_job_lock(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;
    // SAFETY: the handle is live and the initialized synchronous byte-range lock
    // is automatically released when the owning File is closed.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let locked = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if locked != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
fn try_job_lock(_file: &std::fs::File) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Native migration ownership is unavailable on this platform",
    ))
}
