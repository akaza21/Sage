//! The IPC types the frontend sees: projects, workbench tabs and chats,
//! editor documents, repository status, file operations and harnesses. The
//! TypeScript types under `src/domain` mirror these, so their serde forms are
//! part of the contract with the frontend.
//!
//! Invariant: types only. No I/O, no async and no SQLite here, and no other
//! Sage crate but `sage-acp-protocol`. Code that fills these types belongs in
//! the crates above.

mod beta_evidence;
pub mod chat;
pub mod editor;
pub mod file_operations;
pub mod harness;
pub mod project;
pub mod repository;
pub mod resource;
