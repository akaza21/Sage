use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use sage_git::change_stamp::change_stamp;
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::state::AppState;

#[tauri::command]
pub async fn get_repository_change_stamp(state: State<'_, AppState>) -> Result<String, String> {
    // SQLite and the file system both block, so neither runs on the async
    // runtime that this once-a-second poll would otherwise hold up.
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let project_id = database
            .active_project_id()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "no project is active".to_owned())?;
        let mut hasher = DefaultHasher::new();
        for root in database
            .root_records(&project_id)
            .map_err(|error| error.to_string())?
        {
            root.id.hash(&mut hasher);
            change_stamp(&root.absolute_path).hash(&mut hasher);
        }
        Ok(format!("{:016x}", hasher.finish()))
    })
    .await
    .map_err(|error| error.to_string())?
}
