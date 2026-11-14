//! Foundation shared by every Sage crate: the application error type, the
//! process's async runtime, blocking work from async code, wall-clock time,
//! Win32 path spelling and JSON size counting.
//!
//! Invariant: this crate depends on no other Sage crate and never on Tauri or
//! a webview. `rt` holds the app's shared Tokio runtime, which the app hands to
//! Tauri at startup.

pub mod blocking;
pub mod encoded_len;
pub mod error;
pub mod rt;
pub mod time;
pub mod win32_path;
