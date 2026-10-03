fn main() {
    // Tauri's own manifest, plus the setting that keeps Windows from opening
    // a console window when the app is started from Explorer.
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run the Tauri build script");
}
