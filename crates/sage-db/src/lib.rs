//! Sage's SQLite database: the shared write connection, the chat search
//! reader, pinned snapshot readers and the physical storage limits that gate
//! every write. `error` turns rusqlite errors into `AppError::Database`.
//!
//! Invariant: no Tauri, no `AppState` and no feature logic. The schema
//! migration and the startup recovery steps run in the app, which hands this
//! crate a migrated connection; tables belong to the modules that use them.

pub mod database;
pub mod error;
pub mod physical_storage;
