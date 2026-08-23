pub mod acp;
mod app_preview_commands;
mod chat_archive;
mod chat_archive_commands;
mod chat_commands;
mod chat_lifecycle;
mod chat_search;
mod chat_search_commands;
#[cfg(feature = "codex-provisioning-helper")]
#[doc(hidden)]
pub mod codex_helper_boundary;
mod commands;
mod domain;
mod editor;
mod editor_content;
mod editor_coordination;
mod editor_persistence;
mod editor_watch;
mod error;
mod file_create_commands;
mod file_rename_commands;
mod git_process;
mod harnesses;
mod launch_profile;
mod persistence;
mod preview_lifecycle;
mod project_commands;
mod project_migration;
mod project_registry;
mod repository;
mod repository_commit;
mod repository_commit_commands;
mod repository_discovery;
mod repository_index_status;
mod repository_mutation;
mod repository_mutation_commands;
mod repository_parse;
mod repository_public_error;
mod repository_ref_commands;
mod repository_workspace;
mod resource_commands;
mod resource_trash_commands;
mod session_events;
mod state;
mod workbench_persistence;
mod workbench_restore;
mod workspace;

use std::fs;

use tauri::{Manager, RunEvent};
use tracing::info;
#[cfg(any(debug_assertions, feature = "diagnostics"))]
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(all(windows, feature = "acp-fixture"))]
use crate::launch_profile::resolve_characterization_binding;
use crate::{
    launch_profile::resolve_launch_profile, persistence::Database, state::AppState,
    workspace::development_workspace_root,
};

fn handle_exit_request(
    shutdown: impl FnOnce() -> Result<(), crate::acp::runtime::HostError>,
    prevent_exit: impl FnOnce(),
) -> bool {
    if shutdown().is_err() {
        prevent_exit();
        true
    } else {
        false
    }
}

#[cfg(any(debug_assertions, feature = "diagnostics"))]
fn initialize_tracing() {
    let sage_targets = tracing_subscriber::filter::filter_fn(is_sage_target);
    let formatter = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_filter(sage_targets);
    let _ = tracing_subscriber::registry().with(formatter).try_init();
}

#[cfg(any(
    debug_assertions,
    feature = "diagnostics",
    all(test, feature = "acp-fixture")
))]
fn is_sage_target(metadata: &tracing::Metadata<'_>) -> bool {
    let target = metadata.target();
    target == "sage_lib" || target.starts_with("sage_lib::")
}

#[cfg(not(any(debug_assertions, feature = "diagnostics")))]
fn initialize_tracing() {}

pub fn run() {
    initialize_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let disposable_root = std::env::var_os("SAGE_DISPOSABLE_PROFILE_ROOT");
            let disposable_project = std::env::var_os("SAGE_PROJECT_ROOT");
            let profile = resolve_launch_profile(
                disposable_root.as_deref(),
                disposable_project.as_deref(),
                app.path().app_data_dir()?,
                app.path().app_local_data_dir()?,
            )?;
            #[cfg(all(windows, feature = "acp-fixture"))]
            let characterization = resolve_characterization_binding(
                &profile,
                std::env::var_os("SAGE_CHARACTERIZATION_FIXTURE_PATH").as_deref(),
                std::env::var_os("SAGE_CHARACTERIZATION_FIXTURE_SHA256").as_deref(),
                std::env::var_os("SAGE_CHARACTERIZATION_FIXTURE_BYTES").as_deref(),
                std::env::var_os("SAGE_CHARACTERIZATION_CASE").as_deref(),
            )?;
            fs::create_dir_all(profile.durable_dir())?;
            let database = Database::open(&profile.durable_dir().join("sage.db"))?;
            let seed_project_root = if profile.is_disposable() {
                profile.project_root().map(ToOwned::to_owned)
            } else {
                development_workspace_root()
            };
            if database.project_count()? == 0
                && let Some(root) = seed_project_root
            {
                database.create_project_from_root(&root)?;
            }
            #[cfg(all(windows, feature = "acp-fixture"))]
            let state = match characterization {
                Some(binding) => AppState::try_new_characterization_desktop(
                    database,
                    app.handle().clone(),
                    binding,
                )?,
                None => AppState::try_new_desktop(database, app.handle().clone())?,
            };
            #[cfg(not(all(windows, feature = "acp-fixture")))]
            let state = AppState::try_new_desktop(database, app.handle().clone())?;
            app.manage(state);
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
            let main_window = match profile.webview_dir() {
                Some(directory) => main_window.data_directory(directory.to_owned()),
                None => main_window,
            };
            main_window.build()?;
            info!("Sage application state initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            resource_commands::get_bootstrap_snapshot,
            resource_commands::get_repository_status,
            resource_commands::get_file_diff,
            repository_ref_commands::discover_project_repositories,
            repository_ref_commands::get_repository_ref_status,
            repository_ref_commands::get_repository_ref_file_diff,
            editor_watch::watch_editor_document,
            editor_watch::unwatch_editor_document,
            repository_mutation_commands::mutate_git_file,
            repository_commit_commands::commit_git_repository,
            resource_commands::list_directory,
            resource_commands::read_text_file,
            resource_commands::open_editor_document,
            resource_commands::snapshot_editor_buffer,
            resource_commands::save_editor_document,
            resource_commands::discard_editor_buffer,
            file_create_commands::create_project_file,
            file_create_commands::create_project_folder,
            file_rename_commands::rename_project_file,
            file_rename_commands::rename_project_folder,
            resource_trash_commands::trash_project_resource,
            project_commands::create_project_from_picker,
            project_commands::add_project_root_from_picker,
            chat_commands::create_chat,
            chat_commands::activate_chat,
            chat_archive_commands::archive_chat,
            chat_commands::rename_chat,
            chat_archive_commands::restore_chat,
            chat_search_commands::search_project_chats,
            project_commands::persist_project_layout,
            project_commands::persist_project_layout_and_close,
            project_commands::switch_project,
            commands::list_harness_models,
            commands::send_simulated_prompt,
            commands::acp::get_acp_chat_page,
            commands::acp::get_codex_discovery,
            commands::acp::codex_auth::get_codex_auth,
            commands::acp::codex_auth::start_codex_auth,
            commands::acp::codex_auth::cancel_codex_auth,
            commands::acp::codex_auth::logout_codex_auth,
            commands::acp::start_acp_session,
            commands::acp::activate_acp_session,
            commands::acp::send_acp_turn,
            commands::acp::cancel_acp_turn,
            commands::acp::close_acp_session,
            commands::acp::permission::resolve_permission_decision,
            workspace::search::command::search_workspace,
            workspace::search::command::cancel_workspace_search,
            app_preview_commands::discover_preview_targets,
            app_preview_commands::get_preview_health,
            app_preview_commands::start_preview_target,
            app_preview_commands::stop_preview_target
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Sage")
        .run(|app_handle, event| {
            if let RunEvent::ExitRequested { api, .. } = event {
                let state = app_handle.state::<AppState>();
                preview_lifecycle::cleanup_previews_on_exit();
                if handle_exit_request(|| state.acp.shutdown_blocking(), || api.prevent_exit()) {
                    info!("Sage host exit deferred for bounded ACP recovery retry");
                    return;
                }
                info!("Sage host exit requested");
            }
        });
}

pub fn run_workspace_search_helper_if_requested() {
    workspace::search::run_stdio_helper_if_requested();
}

#[cfg(test)]
mod exit_tests {
    use super::*;

    #[test]
    fn exit_is_deferred_until_bounded_shutdown_succeeds() {
        let prevented = std::cell::Cell::new(0);
        assert!(handle_exit_request(
            || Err(crate::acp::runtime::HostError::RecoveryRequired),
            || prevented.set(prevented.get() + 1),
        ));
        assert_eq!(prevented.get(), 1);
        assert!(!handle_exit_request(|| Ok(()), || prevented.set(99)));
        assert_eq!(prevented.get(), 1);
    }
}
