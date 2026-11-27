//! Regenerates the checked-in contract code in `src/gen/` (HS2-XY0VTN). Builds never run
//! protoc: `cargo run -p hotsheet-plugin-protocol --features regen --bin
//! hotsheet-plugin-protocol-regen` rewrites `src/gen/`, and `tests/regen.rs` fails when the
//! checked-in files differ from a fresh generation.

use std::io;
use std::path::Path;

/// The contract files, relative to the crate directory.
pub const PROTO_FILES: &[&str] = &[
    "proto/hotsheet/plugin/v1/envelope.proto",
    "proto/hotsheet/plugin/v1/diagnostics.proto",
];

/// The generated files `generate` writes into `out_dir`.
pub const GENERATED_FILES: &[&str] = &["hotsheet.plugin.v1.rs", "hotsheet.plugin.v1.serde.rs"];

/// Generates prost message types and pbjson canonical-JSON serde impls into `out_dir`.
pub fn generate(crate_dir: &Path, out_dir: &Path) -> io::Result<()> {
    let protoc = protoc_bin_vendored::protoc_bin_path().map_err(io::Error::other)?;
    let scratch = tempfile_dir(out_dir)?;
    let descriptor = scratch.join("descriptor.bin");
    let files: Vec<_> = PROTO_FILES.iter().map(|f| crate_dir.join(f)).collect();
    let mut config = prost_build::Config::new();
    config
        .protoc_executable(protoc)
        .out_dir(out_dir)
        .file_descriptor_set_path(&descriptor);
    config.compile_protos(&files, &[crate_dir.join("proto")])?;
    let bytes = std::fs::read(&descriptor)?;
    std::fs::remove_dir_all(&scratch)?;
    pbjson_build::Builder::new()
        .register_descriptors(&bytes)?
        .out_dir(out_dir)
        .build(&[".hotsheet.plugin.v1"])?;
    // pbjson leaves trailing spaces, which the repository's pre-commit check rejects.
    for file in GENERATED_FILES {
        let path = out_dir.join(file);
        let text = std::fs::read_to_string(&path)?;
        let mut cleaned: String = text
            .lines()
            .map(|line| line.trim_end().to_string() + "\n")
            .collect();
        if cleaned.is_empty() {
            cleaned.push('\n');
        }
        std::fs::write(&path, cleaned)?;
    }
    Ok(())
}

fn tempfile_dir(near: &Path) -> io::Result<std::path::PathBuf> {
    let dir = near.join(".regen-scratch");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
