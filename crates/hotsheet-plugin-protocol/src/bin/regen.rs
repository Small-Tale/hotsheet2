//! Rewrites `src/gen/` from `proto/` (HS2-XY0VTN). See `hotsheet_plugin_protocol::regen`.

fn main() -> std::io::Result<()> {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    hotsheet_plugin_protocol::regen::generate(crate_dir, &crate_dir.join("src/gen"))?;
    println!("regenerated {}", crate_dir.join("src/gen").display());
    Ok(())
}
