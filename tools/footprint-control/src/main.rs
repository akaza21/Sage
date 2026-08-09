#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../../../src-tauri/src/launch_profile.rs"]
mod launch_profile;

use launch_profile::resolve_launch_profile;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let profile = resolve_launch_profile(
                std::env::var_os("SAGE_DISPOSABLE_PROFILE_ROOT").as_deref(),
                None,
                app.path().app_data_dir()?,
                app.path().app_local_data_dir()?,
            )?;
            std::fs::create_dir_all(profile.durable_dir())?;
            if profile.project_root().is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "the footprint control cannot seed a project",
                )
                .into());
            }
            let main_window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|config| config.label == "main")
                .cloned()
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Tauri config is missing the main window",
                    )
                })?;
            let main_window =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &main_window_config)?;
            let main_window = if profile.is_disposable() {
                main_window.data_directory(
                    profile
                        .webview_dir()
                        .expect("disposable profile requires a WebView directory")
                        .to_owned(),
                )
            } else {
                main_window
            };
            main_window.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run the Sage footprint control");
}
