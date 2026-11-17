//! Child process containment shared by Sage's harness, terminal and Git
//! launches: the launch error vocabulary, owned Windows handles, the console
//! a background child gets, the Windows Job Object that bounds a whole process
//! tree, the tree a Git child starts, and the Unix process group a launcher
//! and its children share.
//!
//! Invariant: this crate depends on no other Sage crate and never on Tauri,
//! a webview or the ACP protocol. On Windows a process tree is proven empty
//! only by Job accounting, never by reopening a PID.

#[cfg(windows)]
pub mod console;
pub mod error;
#[cfg(windows)]
pub mod handle;
#[cfg(windows)]
pub mod job;
#[cfg(unix)]
pub mod process_group;
pub mod process_tree;
