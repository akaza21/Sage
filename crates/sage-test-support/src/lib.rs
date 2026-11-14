//! Invariant: only ever a `[dev-dependencies]` entry, so nothing here reaches
//! a shipped binary; it never depends on Tauri or a webview.

pub mod test_diagnostics;
pub mod test_executable;
pub mod test_paths;
pub mod test_wait;
