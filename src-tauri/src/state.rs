use crate::{
    editor_coordination::EditorOperationCoordinator, harnesses::HarnessCatalog,
    persistence::Database,
};

#[derive(Debug)]
pub struct AppState {
    pub database: Database,
    pub harnesses: HarnessCatalog,
    pub editor_operations: EditorOperationCoordinator,
    pub project_transitions: tokio::sync::Mutex<u64>,
}

impl AppState {
    pub fn new(database: Database) -> Self {
        Self {
            database,
            harnesses: HarnessCatalog,
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(0),
        }
    }
}
