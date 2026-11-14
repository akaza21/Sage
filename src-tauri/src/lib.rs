use crate::project_registry::ProjectRegistryStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::bootstrap::BrowserTabNumberStore;
pub mod acp;
pub(crate) mod app_preview_commands;
mod app_run_events;
#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
)]
mod browser;
mod chat_archive;
mod chat_archive_commands;
mod chat_commands;
mod chat_fork;
mod chat_lifecycle;
mod chat_move;
mod chat_pin;
mod chat_purge;
mod chat_queue;
mod chat_queue_commands;
mod chat_read_state;
mod chat_search;
mod chat_search_commands;
#[cfg(feature = "codex-provisioning-helper")]
#[doc(hidden)]
pub mod codex_helper_boundary;
mod commands;
mod database;
mod editor;
mod editor_content;
mod editor_coordination;
mod editor_persistence;
mod editor_watch;
mod file_create_commands;
mod file_rename_commands;
mod harness_discovery;
mod harnesses;
mod helper_entrypoints;
mod image_artifacts;
mod image_attachment;
mod ipc_commands;
mod launch_profile;
mod native_session_import;
mod notifications;
mod preview_lifecycle;
mod profile_handoff;
mod profile_lock;
mod project_commands;
mod project_create_commands;
mod project_migration;
mod project_registry;
mod repository_branch_create_commands;
mod repository_branch_delete_commands;
mod repository_branch_rename_commands;
mod repository_change_stamp_commands;
mod repository_checkout_commands;
mod repository_cherry_pick_commands;
mod repository_commit_commands;
mod repository_git_base_commands;
mod repository_mutation_commands;
mod repository_pull_commands;
mod repository_rebase_commands;
mod repository_ref_commands;
mod repository_remote_transport_commands;
mod repository_revert_commands;
mod repository_sequence_abort_commands;
mod resource_commands;
mod resource_trash_commands;
mod session_events;
mod settings_commands;
mod settings_store;
mod side_chat;
mod side_chat_storage;
mod startup_refusal;
mod state;
mod system_browser;
mod turn_workspace;
pub mod user_terminal;
mod voice_input;
#[cfg(windows)]
mod window_frame;
mod window_memory;
mod window_viewport;
mod workbench_diff_ref;
mod workbench_persistence;
mod workbench_restore;
mod workspace_commands;
mod workspace_notify;

#[cfg(all(feature = "native-lane", not(all(windows, debug_assertions))))]
compile_error!("the native-lane feature boots WebView2 and is for Windows debug builds only");
#[cfg(all(windows, feature = "native-lane"))]
#[doc(hidden)]
pub use browser::native_lane;

use std::{fs, sync::Arc};

use sage_core::error::AppError;
use tauri::{Manager, webview::PageLoadEvent};
use tracing::{info, warn};
#[cfg(any(debug_assertions, feature = "diagnostics"))]
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(all(windows, feature = "acp-fixture"))]
use crate::launch_profile::{DesktopFixtureMode, resolve_desktop_fixture_mode_from_environment};
use crate::{
    launch_profile::{reject_unavailable_fixture_environment_inputs, resolve_launch_profile},
    profile_lock::{ProfileLock, ProfileLockError},
    startup_refusal::Refusal,
    state::AppState,
};
use sage_workspace::development_workspace_root;

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

/// Code outside the app reaches the async runtime through `sage_core::rt`.
/// Hands that runtime to Tauri before anything touches Tauri's, so the process
/// keeps one Tokio runtime. Call it before building any `tauri::Builder`.
pub(crate) fn share_async_runtime() {
    tauri::async_runtime::set(sage_core::rt::handle().clone());
}

pub fn run() {
    initialize_tracing();
    share_async_runtime();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(notifications::init())
        .plugin(window_memory::init())
        .plugin(window_viewport::init())
        .setup(|app| {
            let disposable_root = std::env::var_os("SAGE_DISPOSABLE_PROFILE_ROOT");
            let profile = resolve_launch_profile(
                disposable_root.as_deref(),
                std::env::var_os("SAGE_PROJECT_ROOT").as_deref(),
                app.path().app_data_dir()?,
                app.path().app_local_data_dir()?,
            )?;
            // Nothing below may touch the profile until this process owns it:
            // startup recovery would otherwise rewrite a running Sage's state.
            fs::create_dir_all(profile.durable_dir())?;
            let profile_lock = match ProfileLock::acquire(profile.durable_dir()) {
                Ok(lock) => lock,
                Err(ProfileLockError::Held(_)) => {
                    startup_refusal::exit(Refusal::ProfileInUse, &profile)
                }
                Err(error) => return Err(error.into()),
            };
            app.manage(profile_lock);
            reject_unavailable_fixture_environment_inputs()?;
            #[cfg(all(windows, feature = "acp-fixture"))]
            let fixture_mode = resolve_desktop_fixture_mode_from_environment(&profile)?;
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            let native_fixture_mode = matches!(
                &fixture_mode,
                DesktopFixtureMode::CodexFixture(_)
                    | DesktopFixtureMode::ClaudeFixture(_)
                    | DesktopFixtureMode::PiFixture(_)
            );
            let database =
                match crate::database::open_database(&profile.durable_dir().join("sage.db")) {
                    Ok(database) => database,
                    Err(error @ AppError::NewerProfile { .. }) => {
                        startup_refusal::exit(Refusal::NewerProfile(error.to_string()), &profile)
                    }
                    Err(error) => return Err(error.into()),
                };
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
            let settings =
                settings_store::SettingsStore::open(&profile.durable_dir().join("settings.db"))?;
            browser_state
                .attach_site_settings(settings.clone())
                .map_err(std::io::Error::other)?;
            app.manage(browser_state);
            app.manage(project_create_commands::ProjectSourcePicks::default());
            app.manage(browser::passwords::PasswordsState::new(
                profile.durable_dir(),
            ));
            #[cfg(all(windows, feature = "acp-fixture"))]
            let state = match fixture_mode {
                DesktopFixtureMode::GenericCharacterization(binding) => {
                    AppState::try_new_characterization_desktop(
                        database,
                        settings,
                        app.handle().clone(),
                        binding,
                    )?
                }
                #[cfg(debug_assertions)]
                DesktopFixtureMode::CodexFixture(selection) => {
                    AppState::try_new_codex_fixture_desktop(
                        database,
                        settings,
                        app.handle().clone(),
                        selection,
                    )?
                }
                #[cfg(debug_assertions)]
                DesktopFixtureMode::ClaudeFixture(selection) => {
                    AppState::try_new_claude_fixture_desktop(
                        database,
                        settings,
                        app.handle().clone(),
                        selection,
                    )?
                }
                #[cfg(debug_assertions)]
                DesktopFixtureMode::PiFixture(selection) => AppState::try_new_pi_fixture_desktop(
                    database,
                    settings,
                    app.handle().clone(),
                    selection,
                )?,
                DesktopFixtureMode::Ordinary => {
                    AppState::try_new_desktop(database, settings, app.handle().clone())?
                }
            };
            #[cfg(not(all(windows, feature = "acp-fixture")))]
            let state = AppState::try_new_desktop(database, settings, app.handle().clone())?;
            app.manage(state);
            notifications::start(app.handle());
            #[cfg_attr(not(windows), allow(clippy::default_constructed_unit_structs))]
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
                            let start = renderer_recreation
                                .begin_start(&renderer_recreation_app.state::<AppState>());
                            let lifecycle = Arc::clone(&renderer_recreation);
                            let app = renderer_recreation_app.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                lifecycle.capture_start(&app.state::<AppState>(), start);
                            });
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
            #[cfg(windows)]
            let main_window = if main_window_config.decorations {
                main_window
            } else {
                main_window.initialization_script(window_frame::initialization_script())
            };
            let main_window = match profile.webview_dir() {
                Some(directory) => main_window.data_directory(directory.to_owned()),
                None => main_window,
            };
            main_window.build()?;
            if !profile.is_disposable()
                && let Err(error) = profile_handoff::listen(app.handle(), profile.durable_dir())
            {
                warn!(
                    ?error,
                    "a second launch will not bring this Sage to the front"
                );
            }
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            if !native_fixture_mode {
                AppState::refresh_harnesses_after_window_shown(app.handle().clone());
            }
            #[cfg(not(all(windows, feature = "acp-fixture", debug_assertions)))]
            AppState::refresh_harnesses_after_window_shown(app.handle().clone());
            AppState::release_idle_runtimes_periodically(app.handle().clone());
            info!("Sage application state initialized");
            Ok(())
        })
        .invoke_handler(ipc_commands::handle_main_invoke)
        .build(tauri::generate_context!())
        .expect("failed to build Sage")
        .run(app_run_events::handle_run_event);
}

pub use helper_entrypoints::{
    run_browser_mcp_helper_if_requested, run_workspace_search_helper_if_requested,
};
