pub mod acp;
pub(crate) mod app_preview_commands;
mod app_run_events;
mod browser;
mod chat_archive;
mod chat_archive_commands;
mod chat_commands;
mod chat_lifecycle;
mod chat_move;
mod chat_pin;
mod chat_queue;
mod chat_queue_commands;
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
mod git_budget;
mod git_process;
mod harness_discovery;
mod harnesses;
mod helper_entrypoints;
mod launch_profile;
mod persistence;
mod preview_lifecycle;
mod project_commands;
mod project_migration;
mod project_registry;
mod repository;
mod repository_branch_create;
mod repository_branch_create_commands;
mod repository_branch_delete;
mod repository_branch_delete_commands;
mod repository_branch_rename;
mod repository_branch_rename_commands;
mod repository_branches;
mod repository_checkout;
mod repository_checkout_commands;
mod repository_cherry_pick_commands;
mod repository_commit;
mod repository_commit_commands;
mod repository_conflict;
mod repository_diff;
mod repository_discovery;
mod repository_git;
mod repository_history;
mod repository_index_status;
mod repository_merge_commit;
mod repository_mutation;
mod repository_mutation_commands;
mod repository_parse;
mod repository_public_error;
mod repository_pull;
mod repository_pull_commands;
mod repository_rebase_commands;
mod repository_ref_commands;
mod repository_remote_transport;
mod repository_remote_transport_commands;
mod repository_remotes;
mod repository_revert;
mod repository_revert_commands;
mod repository_sequence_abort;
mod repository_sequence_abort_commands;
mod repository_status;
mod repository_workspace;
mod resource_commands;
mod resource_trash_commands;
mod session_events;
mod settings_commands;
mod settings_store;
mod state;
#[cfg(test)]
mod test_wait;
pub mod user_terminal;
mod voice_input;
mod workbench_diff_ref;
mod workbench_persistence;
mod workbench_restore;
mod workspace;

use std::{fs, sync::Arc};

use tauri::{Manager, webview::PageLoadEvent};
use tracing::{info, warn};
#[cfg(any(debug_assertions, feature = "diagnostics"))]
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(all(windows, feature = "acp-fixture"))]
use crate::launch_profile::resolve_characterization_binding;
use crate::{
    launch_profile::resolve_launch_profile, persistence::Database, state::AppState,
    workspace::development_workspace_root,
};

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
            let profile = resolve_launch_profile(
                disposable_root.as_deref(),
                std::env::var_os("SAGE_PROJECT_ROOT").as_deref(),
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
            let active_project_roots = database
                .repair_active_project()?
                .map(|id| database.root_records(&id))
                .transpose()?
                .unwrap_or_default()
                .into_iter()
                .map(|record| record.absolute_path)
                .collect::<Vec<_>>();
            let next_browser_tab_number = database.next_browser_tab_number()?;
            let browser_state = browser::BrowserState::new(
                &profile,
                app.path().app_local_data_dir()?,
                &active_project_roots,
                next_browser_tab_number,
            )
            .map_err(std::io::Error::other)?;
            app.manage(browser_state);
            #[cfg(all(windows, feature = "acp-fixture"))]
            let state = match characterization {
                Some(binding) => AppState::try_new_characterization_desktop(
                    database,
                    settings_store::SettingsStore::open(
                        &profile.durable_dir().join("settings.db"),
                    )?,
                    app.handle().clone(),
                    binding,
                )?,
                None => AppState::try_new_desktop(
                    database,
                    settings_store::SettingsStore::open(
                        &profile.durable_dir().join("settings.db"),
                    )?,
                    app.handle().clone(),
                )?,
            };
            #[cfg(not(all(windows, feature = "acp-fixture")))]
            let state = AppState::try_new_desktop(
                database,
                settings_store::SettingsStore::open(&profile.durable_dir().join("settings.db"))?,
                app.handle().clone(),
            )?;
            app.manage(state);
            app.manage(voice_input::VoiceInputState::default());
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
            let renderer_recreation =
                Arc::new(crate::acp::service::RendererRecreationReloadLifecycle::default());
            let renderer_recreation_app = app.handle().clone();
            let main_window =
                tauri::WebviewWindowBuilder::from_config(app.handle(), &main_window_config)?
                    .on_page_load(move |window, payload| match payload.event() {
                        PageLoadEvent::Started => {
                            let state = renderer_recreation_app.state::<AppState>();
                            renderer_recreation.record_admitted_start(&state);
                        }
                        PageLoadEvent::Finished => {
                            let lifecycle = Arc::clone(&renderer_recreation);
                            let app = renderer_recreation_app.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                let state = app.state::<AppState>();
                                if let Some(result) =
                                    lifecycle.pair_admitted_finish(&state, &window)
                                {
                                    match result {
                                        Ok(()) => info!("ACP renderer recreation pairing passed"),
                                        Err(error) => {
                                            warn!(?error, "ACP renderer recreation pairing failed")
                                        }
                                    }
                                }
                            });
                        }
                    });
            let main_window = match profile.webview_dir() {
                Some(directory) => main_window.data_directory(directory.to_owned()),
                None => main_window,
            };
            main_window.build()?;
            AppState::refresh_harnesses_after_window_shown(app.handle().clone());
            info!("Sage application state initialized");
            Ok(())
        })
        .invoke_handler(|invoke: tauri::ipc::Invoke| {
            let webview = invoke.message.webview_ref();
            let admitted = webview
                .url()
                .is_ok_and(|url| browser::ipc_guard::admits_main_invoke(webview.label(), &url));
            if !admitted {
                invoke
                    .resolver
                    .reject("Sage commands are available only to the main app webview");
                return true;
            }
            let handle: fn(tauri::ipc::Invoke) -> bool = tauri::generate_handler![
                user_terminal::ipc::terminal_create,
                user_terminal::ipc::terminal_write,
                user_terminal::ipc::terminal_resize,
                user_terminal::ipc::terminal_read,
                user_terminal::ipc::terminal_close,
                resource_commands::get_bootstrap_snapshot,
                voice_input::start_voice_input,
                voice_input::stop_voice_input,
                resource_commands::get_repository_status,
                resource_commands::get_file_diff,
                repository_ref_commands::discover_project_repositories,
                repository_ref_commands::get_repository_ref_status,
                repository_ref_commands::get_repository_ref_file_diff,
                editor_watch::watch_editor_document,
                editor_watch::unwatch_editor_document,
                workspace::notify::watch_workspace_notify,
                workspace::notify::unwatch_workspace_notify,
                editor::language::start_language_session,
                editor::language::cancel_language_session,
                repository_mutation_commands::mutate_git_file,
                repository_commit_commands::commit_git_repository,
                repository_checkout_commands::checkout_git_local_branch,
                repository_branch_create_commands::create_git_local_branch,
                repository_branch_delete_commands::delete_git_local_branch,
                repository_branch_rename_commands::rename_git_local_branch,
                repository_sequence_abort_commands::abort_git_in_progress,
                repository_cherry_pick_commands::continue_git_cherry_pick,
                repository_rebase_commands::continue_git_rebase,
                repository_revert_commands::continue_git_revert,
                repository_remote_transport_commands::fetch_git_remote,
                repository_remote_transport_commands::push_git_remote,
                repository_pull_commands::pull_git_repository,
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
                project_commands::ensure_local_project,
                chat_commands::create_chat,
                chat_commands::move_chat,
                chat_commands::pin_chat,
                chat_commands::activate_chat,
                chat_archive_commands::archive_chat,
                chat_commands::rename_chat,
                chat_archive_commands::restore_chat,
                chat_search_commands::search_project_chats,
                chat_queue_commands::get_project_chat_queued_prompts,
                chat_queue_commands::replace_chat_queued_prompts,
                project_commands::persist_project_layout,
                project_commands::persist_project_layout_and_close,
                project_commands::trace_close_stage,
                project_commands::switch_project,
                commands::list_harness_models,
                commands::list_harnesses,
                commands::send_simulated_prompt,
                settings_commands::get_editor_appearance,
                settings_commands::set_editor_appearance,
                settings_commands::get_session_access_mode,
                settings_commands::set_session_access_mode,
                settings_commands::get_ui_appearance,
                settings_commands::set_ui_appearance,
                commands::acp::get_acp_chat_page,
                commands::acp::get_acp_file_snapshot,
                commands::acp::file_reference::resolve_chat_file_reference,
                commands::acp::get_codex_discovery,
                #[cfg(any(debug_assertions, feature = "diagnostics"))]
                commands::acp::runtime_identity::get_codex_runtime_identity,
                commands::acp::codex_auth::get_codex_auth,
                commands::acp::codex_auth::start_codex_auth,
                commands::acp::codex_auth::cancel_codex_auth,
                commands::acp::codex_auth::logout_codex_auth,
                commands::acp::start_acp_session,
                commands::acp::activate_acp_session,
                commands::acp::send_acp_turn,
                commands::acp::cancel_acp_turn,
                commands::acp::steer_acp_turn,
                commands::acp::close_acp_session,
                commands::acp::end_acp_session,
                commands::acp::session_config::set_acp_session_mode,
                commands::acp::session_config::set_acp_session_config_option,
                commands::acp::permission::resolve_permission_decision,
                commands::acp::read_text_file::acknowledge_acp_read_text_file,
                commands::acp::edit_disposition::record_agent_edit_disposition,
                workspace::search::command::search_workspace,
                workspace::search::command::cancel_workspace_search,
                app_preview_commands::discover_preview_targets,
                app_preview_commands::get_preview_health,
                app_preview_commands::start_preview_target,
                app_preview_commands::stop_preview_target,
                browser::lifecycle::browser_open,
                acp::browser::attachment::attach_browser_tab,
                acp::browser::attachment::detach_browser_tab,
                acp::browser::attachment::stop_browser_tab,
                acp::browser::attachment::browser_attachment_status,
                browser::lifecycle::browser_close,
                browser::clear_data::browser_clear_data,
                browser::bounds::browser_set_bounds,
                browser::page_controls::browser_hide,
                browser::downloads::browser_downloads,
                browser::downloads::browser_cancel_download,
                browser::downloads::show::browser_show_download_in_folder,
                browser::site_permissions::browser_permission_status,
                browser::site_permissions::browser_permission_revoke,
                browser::site_permissions::browser_permission_resolve,
                browser::navigation::browser_status,
                browser::state_query::browser_live_tabs,
                browser::state_query::browser_history,
                browser::state_query::browser_suggested_sites,
                browser::page_context::browser_page_context,
                browser::page_controls::browser_navigate,
                browser::navigation::browser_control,
                browser::page_controls::browser_zoom,
                browser::page_controls::browser_open_external,
                browser::page_controls::browser_find
            ];
            handle(invoke)
        })
        .build(tauri::generate_context!())
        .expect("failed to build Sage")
        .run(app_run_events::handle_run_event);
}

pub use helper_entrypoints::{
    run_browser_mcp_helper_if_requested, run_workspace_search_helper_if_requested,
};
