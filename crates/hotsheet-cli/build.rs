#[path = "../hotsheet-server/src/source_revision.rs"]
#[allow(dead_code)]
mod source_revision;

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=HOT_SHEET_BUILD_REVISION");
    for path in [
        "Cargo.toml",
        "build.rs",
        "src",
        "../../Cargo.toml",
        "../../Cargo.lock",
        "../../crates",
        "../../plugins",
        "../../.claude/skills/hotsheet",
        "../../.agents/skills/hotsheet",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    if let Some(revision) =
        std::env::var_os("HOT_SHEET_BUILD_REVISION").filter(|value| !value.is_empty())
    {
        println!(
            "cargo:rustc-env=HOT_SHEET_BUILD_REVISION={}",
            revision.to_string_lossy()
        );
        return;
    }

    let root =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir")).join("../..");
    let revision = source_revision::revision_for_source_root(&root)
        .expect("hash Hot Sheet workspace source for the local CLI build");
    println!("cargo:rustc-env=HOT_SHEET_BUILD_REVISION={revision}");
    println!(
        "cargo:rustc-env=HOT_SHEET_LOCAL_SOURCE_ROOT={}",
        root.display()
    );
}
