fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new()
                .commands(&["desktop_flush_complete", "desktop_hide_cursor_until_moves"]),
        ),
    )
    .expect("failed to build desktop permissions");
}
