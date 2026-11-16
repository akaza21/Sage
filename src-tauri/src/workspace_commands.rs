//! The workspace IPC commands. Each reads the active Project from the app's
//! state and calls `sage_workspace` (or the watcher in `workspace_notify`)
//! with explicit arguments.

pub(crate) mod notify;
pub(crate) mod quick_open;
pub(crate) mod replace;
pub(crate) mod search;
