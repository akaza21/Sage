//! Child process containment shared by Sage's harness, terminal and Git
//! launches: the launch error vocabulary, owned Windows handles, the Windows
//! Job Object that bounds a whole process tree, and the tree a Git child
//! starts.
//!
//! Invariant: this crate depends on no other Sage crate and never on Tauri,
//! a webview or the ACP protocol. On Windows a process tree is proven empty
//! only by Job accounting, never by reopening a PID.

pub mod error;
#[cfg(windows)]
pub mod handle;
#[cfg(windows)]
pub mod job;
pub mod process_tree;
