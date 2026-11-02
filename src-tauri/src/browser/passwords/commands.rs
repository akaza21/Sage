//! Passwords page commands (BF-7b-1). They run only for the main webview:
//! the invoke guard in `ipc_commands.rs` rejects every browser tab first.
//! Replies carry ids, sites, usernames and counts. Only Reveal returns a
//! password, and Reveal and Copy first ask the OS to confirm the user.

use serde::Serialize;
use tauri::{AppHandle, State, Window};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use uuid::Uuid;

use super::{
    PasswordsState, VaultError, VaultResult, clipboard, confirm::confirm_user, secret::Secret,
    state::PasswordsOverview, store::MergeCounts,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportReport {
    #[serde(flatten)]
    counts: MergeCounts,
    skipped: u32,
    file_name: String,
}

async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> VaultResult<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// Ids are the vault's UUIDs; anything else names no saved password.
fn credential_id(id: &str) -> Result<String, String> {
    Uuid::parse_str(id)
        .map(|id| id.to_string())
        .map_err(|_| VaultError::NotFound.to_string())
}

#[tauri::command]
pub(crate) async fn browser_passwords(
    state: State<'_, PasswordsState>,
    query: String,
) -> Result<PasswordsOverview, String> {
    let passwords = state.inner().clone();
    blocking(move || passwords.overview(&query)).await
}

/// Picks a CSV in the native file dialog and imports it. The path never
/// crosses IPC; `None` means the dialog or the import was cancelled.
#[tauri::command]
pub(crate) async fn browser_passwords_import_csv(
    app: AppHandle,
    window: Window,
    state: State<'_, PasswordsState>,
) -> Result<Option<ImportReport>, String> {
    // Admitted before the dialog opens, so Cancel reaches it from the start.
    let run = state.begin_import().map_err(|error| error.to_string())?;
    let dialog = app
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Import passwords from a CSV file")
        .add_filter("CSV", &["csv"]);
    blocking(move || {
        let Some(selected) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        if run.cancelled() {
            return Ok(None);
        }
        let path = selected
            .into_path()
            .map_err(|_| VaultError::Invalid("Choose a file on this computer."))?;
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (counts, skipped) = match run.import(path) {
            Err(VaultError::Cancelled) => return Ok(None),
            imported => imported?,
        };
        Ok(Some(ImportReport {
            counts,
            skipped,
            file_name,
        }))
    })
    .await
}

/// True means cancellation was accepted and the import will roll back.
/// False means commit was already admitted or the import had already ended.
#[tauri::command]
pub(crate) async fn browser_passwords_cancel_import(
    state: State<'_, PasswordsState>,
) -> Result<bool, String> {
    let cancel = state.cancel_import_request();
    blocking(move || Ok(cancel())).await
}

#[tauri::command]
pub(crate) async fn browser_passwords_delete_imported_file(
    state: State<'_, PasswordsState>,
) -> Result<(), String> {
    let passwords = state.inner().clone();
    blocking(move || passwords.delete_imported_file()).await
}

#[tauri::command]
pub(crate) async fn browser_passwords_update(
    state: State<'_, PasswordsState>,
    id: String,
    username: String,
    password: Option<Secret>,
) -> Result<(), String> {
    let id = credential_id(&id)?;
    let passwords = state.inner().clone();
    blocking(move || passwords.update(&id, &username, password.as_ref())).await
}

#[tauri::command]
pub(crate) async fn browser_passwords_delete(
    state: State<'_, PasswordsState>,
    id: String,
) -> Result<(), String> {
    let id = credential_id(&id)?;
    let passwords = state.inner().clone();
    blocking(move || passwords.delete(&id)).await
}

#[tauri::command]
pub(crate) async fn browser_passwords_delete_all(
    state: State<'_, PasswordsState>,
) -> Result<(), String> {
    let passwords = state.inner().clone();
    blocking(move || passwords.delete_all()).await
}

#[tauri::command]
pub(crate) async fn browser_passwords_reveal(
    window: Window,
    state: State<'_, PasswordsState>,
    id: String,
) -> Result<String, String> {
    let id = credential_id(&id)?;
    let passwords = state.inner().clone();
    blocking(move || {
        passwords.check_readable(&id)?;
        confirm_user(&window, "Sage wants to show a saved password.")?;
        Ok(passwords.password(&id)?.expose().to_owned())
    })
    .await
}

#[tauri::command]
pub(crate) async fn browser_passwords_copy(
    window: Window,
    state: State<'_, PasswordsState>,
    id: String,
) -> Result<(), String> {
    let id = credential_id(&id)?;
    let passwords = state.inner().clone();
    let owner = window.clone();
    let copied = blocking(move || {
        passwords.check_readable(&id)?;
        confirm_user(&window, "Sage wants to copy a saved password.")?;
        clipboard::copy_concealed(&window, passwords.password(&id)?)
    })
    .await?;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(clipboard::CLEAR_AFTER).await;
        let reporter = owner.clone();
        let cleared = tauri::async_runtime::spawn_blocking(move || {
            clipboard::expire(
                &copied,
                |copied| clipboard::clear_if_unchanged(&owner, copied),
                |message| clipboard_warning(&owner, message),
            );
        })
        .await;
        if let Err(error) = cleared {
            clipboard::report_failure(&error.to_string(), |message| {
                clipboard_warning(&reporter, message);
            });
        }
    });
    Ok(())
}

fn clipboard_warning(owner: &Window, message: &str) {
    owner
        .dialog()
        .message(message)
        .title("Sage clipboard cleanup failed")
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}
