use crate::domain::ResourceRef;

#[tauri::command]
pub fn start_language_session(resource: ResourceRef) -> Result<u32, String> {
    let _ = resource;
    Err("language session is not implemented".into())
}

#[tauri::command]
pub fn cancel_language_session(resource: ResourceRef) -> Result<(), String> {
    let _ = resource;
    Err("language session is not implemented".into())
}
