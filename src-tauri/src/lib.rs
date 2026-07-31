mod commands;
mod domain;
mod editor;
mod editor_content;
mod editor_coordination;
mod editor_persistence;
mod error;
mod harnesses;
mod persistence;
mod project_commands;
mod project_migration;
mod project_registry;
mod repository;
mod repository_parse;
mod resource_commands;
mod session_events;
mod state;
mod workbench_persistence;
mod workbench_restore;
mod workspace;

use std::fs;
#[cfg(feature = "diagnostics")]
use std::path::PathBuf;

use tauri::{Manager, RunEvent};
use tracing::info;
#[cfg(any(debug_assertions, feature = "diagnostics"))]
use tracing_subscriber::EnvFilter;

use crate::{persistence::Database, state::AppState, workspace::development_workspace_root};

#[cfg(any(debug_assertions, feature = "diagnostics"))]
fn initialize_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("sage=info")),
        )
        .with_target(false)
        .try_init();
}

#[cfg(not(any(debug_assertions, feature = "diagnostics")))]
fn initialize_tracing() {}

#[cfg(feature = "diagnostics")]
fn diagnostic_data_dir() -> std::io::Result<Option<PathBuf>> {
    let Some(configured) = std::env::var_os("SAGE_DIAGNOSTIC_DATA_DIR") else {
        return Ok(None);
    };
    let configured = PathBuf::from(configured);
    if !configured.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "SAGE_DIAGNOSTIC_DATA_DIR must be absolute",
        ));
    }
    Ok(Some(configured))
}

pub fn run() {
    initialize_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(feature = "diagnostics")]
            let app_data_dir = diagnostic_data_dir()?.unwrap_or(app.path().app_data_dir()?);
            #[cfg(not(feature = "diagnostics"))]
            let app_data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&app_data_dir)?;
            let database = Database::open(&app_data_dir.join("sage.db"))?;
            if database.project_count()? == 0
                && let Some(root) = development_workspace_root()
            {
                database.create_project_from_root(&root)?;
            }
            app.manage(AppState::new(database));
            info!(path = %app_data_dir.display(), "Sage application state initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            resource_commands::get_bootstrap_snapshot,
            resource_commands::get_repository_status,
            resource_commands::get_file_diff,
            resource_commands::list_directory,
            resource_commands::read_text_file,
            resource_commands::open_editor_document,
            resource_commands::snapshot_editor_buffer,
            resource_commands::save_editor_document,
            resource_commands::discard_editor_buffer,
            project_commands::create_project_from_picker,
            project_commands::add_project_root_from_picker,
            project_commands::persist_project_layout,
            project_commands::persist_project_layout_and_close,
            project_commands::switch_project,
            commands::list_harness_models,
            commands::send_simulated_prompt
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Sage")
        .run(|_app_handle, event| {
            if matches!(event, RunEvent::ExitRequested { .. }) {
                info!("Sage host exit requested");
            }
        });
}
