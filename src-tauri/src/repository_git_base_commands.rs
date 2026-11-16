use sage_git::git_base::{FileGitBase, file_git_base};
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::state::AppState;
use sage_domain::resource::ResourceRef;

#[tauri::command]
pub async fn get_file_git_base(
    state: State<'_, AppState>,
    resource: ResourceRef,
    known_oid: Option<String>,
) -> Result<FileGitBase, String> {
    let workspace = state
        .database
        .resolve_active_resource(&resource)
        .map_err(|error| error.to_string())?;
    file_git_base(&workspace, &resource, known_oid.as_deref())
        .await
        .map_err(|error| error.to_string())
}
