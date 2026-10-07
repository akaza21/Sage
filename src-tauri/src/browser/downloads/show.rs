use std::path::Path;

use tauri::State;

use super::super::BrowserState;

#[tauri::command]
pub(crate) async fn browser_show_download_in_folder(
    state: State<'_, BrowserState>,
    tab_id: String,
    id: u64,
) -> Result<(), String> {
    if !state
        .tabs
        .lock()
        .map_err(|error| error.to_string())?
        .entries
        .contains_key(&tab_id)
    {
        return Err("browser tab unavailable".into());
    }
    let folder = state
        .downloads
        .lock()
        .map_err(|error| error.to_string())?
        .folder_for_completed(&tab_id, id)
        .ok_or("completed browser download unavailable")?;
    tauri::async_runtime::spawn_blocking(move || open_folder(&folder))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(windows)]
fn open_folder(folder: &Path) -> Result<(), String> {
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::PCWSTR,
    };

    let operation = "open\0".encode_utf16().collect::<Vec<_>>();
    let target = folder
        .to_str()
        .filter(|path| !path.contains('\0'))
        .ok_or("invalid download destination")?;
    let target = format!("{target}\0").encode_utf16().collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err("could not show browser download folder".into())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn open_folder(_folder: &Path) -> Result<(), String> {
    Err("showing browser download folder is unavailable on this platform".into())
}
