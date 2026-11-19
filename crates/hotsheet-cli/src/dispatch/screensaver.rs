//! `hotsheet-cli screensaver`: the headless install, status, and removal of the macOS screen saver
//! (HS2-MZM988, `docs/34-glance-surfaces.md` §34.12). The desktop app runs the same workflow, so
//! both clients share it. The shared file operations live in `hotsheet_ticketing::screensaver`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use hotsheet_ticketing::screensaver::{self, Locations, PasswordSetting};

#[derive(Subcommand)]
pub(crate) enum ScreensaverCmd {
    /// Copy the Hot Sheet screen saver into ~/Library/Screen Savers. Idempotent: a newer build
    /// replaces the old one whole. Then choose it in System Settings > Screen Saver.
    Install {
        /// The built `HotSheet.saver` (default: the desktop app's copy, else a development build).
        #[arg(long)]
        bundle: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Report whether the saver is installed and fed, and whether macOS asks for the account
    /// password when it ends, with the exact place to turn that on.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Remove the saver and its snapshot. A running server stops writing the snapshot.
    Uninstall {
        #[arg(long)]
        json: bool,
    },
}

/// Where a bundle may be found without being told: the desktop app's resources, then a development
/// build, relative to this executable.
fn bundle_candidates(current_exe: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = current_exe.parent() {
        // `Hot Sheet 2.app/Contents/MacOS/hotsheet-cli` beside `Contents/Resources`.
        candidates.push(
            dir.join("..")
                .join("Resources")
                .join("screensaver")
                .join(screensaver::BUNDLE_NAME),
        );
        // `<repo>/target/<profile>/hotsheet-cli` beside `<repo>/clients/macos-screensaver`.
        candidates.push(
            dir.join("..")
                .join("..")
                .join("clients")
                .join("macos-screensaver")
                .join("build")
                .join(screensaver::BUNDLE_NAME),
        );
    }
    candidates
}

fn find_bundle(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let current = std::env::current_exe().context("cannot locate hotsheet-cli")?;
    let candidates = bundle_candidates(&current);
    candidates
        .iter()
        .find(|path| path.join("Contents/Info.plist").is_file())
        .cloned()
        .with_context(|| {
            format!(
                "no HotSheet.saver found next to this CLI. Build one with \
                 `clients/macos-screensaver/build.sh` (after `npm run build:saver` in clients/web) \
                 and pass it with --bundle. Looked in: {}",
                candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn locations() -> Result<Locations> {
    screensaver::locations_from_env().context("cannot find the home directory")
}

fn describe_password(password: PasswordSetting) -> String {
    match password {
        PasswordSetting::Immediately => "required immediately".to_string(),
        PasswordSetting::AfterSeconds(seconds) => format!("required after {seconds} seconds"),
        PasswordSetting::Off => "not required".to_string(),
        PasswordSetting::Unknown => "unknown".to_string(),
    }
}

pub(crate) fn run(cmd: ScreensaverCmd) -> Result<()> {
    let json = match &cmd {
        ScreensaverCmd::Install { json, .. }
        | ScreensaverCmd::Status { json }
        | ScreensaverCmd::Uninstall { json } => *json,
    };
    if !cfg!(target_os = "macos") {
        if json {
            println!("{}", serde_json::json!({ "supported": false }));
        } else {
            println!("The Hot Sheet screen saver is for macOS only.");
        }
        return Ok(());
    }
    let locations = locations()?;
    match cmd {
        ScreensaverCmd::Install { bundle, .. } => {
            let source = find_bundle(bundle)?;
            let installed = screensaver::install(&locations, &source)
                .with_context(|| format!("installing {}", source.display()))?;
            let status =
                screensaver::status(&locations, true, screensaver::read_password_setting());
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "replaced": installed.replaced,
                        "bundle": installed.bundle,
                        "snapshot_dirs": installed.snapshot_dirs,
                        "next_steps": status.next_steps(),
                    }))?
                );
            } else {
                let verb = if installed.replaced {
                    "Updated"
                } else {
                    "Installed"
                };
                println!(
                    "{verb} the Hot Sheet screen saver: {}",
                    installed.bundle.display()
                );
                if installed.snapshot_dirs.is_empty() {
                    println!(
                        "  The snapshot folder appears the first time macOS runs the saver; a running server then feeds it."
                    );
                } else {
                    println!("  A running server feeds it within a minute.");
                }
                for step in status.next_steps() {
                    println!("  {step}");
                }
            }
        }
        ScreensaverCmd::Status { .. } => {
            let status =
                screensaver::status(&locations, true, screensaver::read_password_setting());
            if json {
                let mut value = serde_json::to_value(&status)?;
                value["next_steps"] = serde_json::to_value(status.next_steps())?;
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                let yes_no = |value: bool| if value { "yes" } else { "no" };
                println!(
                    "Hot Sheet screen saver: {}",
                    if status.installed {
                        "installed"
                    } else {
                        "not installed"
                    }
                );
                println!("  bundle:    {}", status.bundle.display());
                println!(
                    "  snapshot:  {}",
                    if status.snapshot_present {
                        "present"
                    } else {
                        "none yet"
                    }
                );
                println!(
                    "  folders:   {}",
                    if status.snapshot_dirs.is_empty() {
                        "none (created when macOS first runs the saver)".to_string()
                    } else {
                        status
                            .snapshot_dirs
                            .iter()
                            .map(|dir| dir.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                );
                println!("  password:  {}", describe_password(status.password));
                println!(
                    "  password required after the saver: {}",
                    match status.password.requires_password() {
                        Some(value) => yes_no(value),
                        None => "unknown",
                    }
                );
                for step in status.next_steps() {
                    println!("  next: {step}");
                }
            }
        }
        ScreensaverCmd::Uninstall { .. } => {
            let removed = screensaver::uninstall(&locations)?;
            if json {
                println!("{}", serde_json::json!({ "removed": removed }));
            } else if removed {
                println!(
                    "Removed the Hot Sheet screen saver. Choose another saver in System Settings if it was selected."
                );
            } else {
                println!("The Hot Sheet screen saver was not installed.");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_are_looked_for_in_the_app_then_a_development_build() {
        let found = bundle_candidates(Path::new(
            "/Apps/Hot Sheet 2.app/Contents/MacOS/hotsheet-cli",
        ));
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("../Resources/screensaver/HotSheet.saver"));
        assert!(found[1].ends_with("../../clients/macos-screensaver/build/HotSheet.saver"));
        assert!(bundle_candidates(Path::new("hotsheet-cli")).len() == 2);
    }

    #[test]
    fn an_explicit_bundle_is_taken_as_given_and_a_missing_one_names_where_it_looked() {
        assert_eq!(
            find_bundle(Some(PathBuf::from("/x/HotSheet.saver"))).unwrap(),
            PathBuf::from("/x/HotSheet.saver")
        );
        let message = format!("{:#}", find_bundle(None).unwrap_err());
        assert!(
            message.contains("build.sh") && message.contains("--bundle"),
            "{message}"
        );
    }

    #[test]
    fn the_password_reads_in_plain_words() {
        assert_eq!(
            describe_password(PasswordSetting::Immediately),
            "required immediately"
        );
        assert_eq!(
            describe_password(PasswordSetting::AfterSeconds(300)),
            "required after 300 seconds"
        );
        assert_eq!(describe_password(PasswordSetting::Off), "not required");
        assert_eq!(describe_password(PasswordSetting::Unknown), "unknown");
    }
}
