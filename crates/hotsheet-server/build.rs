#[path = "src/source_revision.rs"]
#[allow(dead_code)]
mod source_revision;

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=HOT_SHEET_BUILD_REVISION");
    println!("cargo:rerun-if-env-changed=HOTSHEET_GITHUB_APP_CLIENT_ID");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=github-app-client-id.txt");
    println!("cargo:rerun-if-changed=src");

    let bundled_github_client_id = std::env::var("HOTSHEET_GITHUB_APP_CLIENT_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| include_str!("github-app-client-id.txt").trim().to_owned());
    assert!(
        valid_github_client_id(&bundled_github_client_id),
        "Hot Sheet GitHub App Client ID is missing or invalid; set HOTSHEET_GITHUB_APP_CLIENT_ID or restore crates/hotsheet-server/github-app-client-id.txt"
    );
    println!("cargo:rustc-env=HOTSHEET_BUNDLED_GITHUB_APP_CLIENT_ID={bundled_github_client_id}");

    if let Some(revision) =
        std::env::var_os("HOT_SHEET_BUILD_REVISION").filter(|value| !value.is_empty())
    {
        println!(
            "cargo:rustc-env=HOT_SHEET_BUILD_REVISION={}",
            revision.to_string_lossy()
        );
        return;
    }

    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let revision = source_revision::revision_for_source_root(&root)
        .expect("hash hotsheet-server source for the local build");
    println!("cargo:rustc-env=HOT_SHEET_BUILD_REVISION={revision}");
    println!(
        "cargo:rustc-env=HOT_SHEET_LOCAL_SOURCE_ROOT={}",
        root.display()
    );
}

fn valid_github_client_id(value: &str) -> bool {
    value.starts_with("Iv")
        && value.len() >= 12
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}
