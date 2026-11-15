//! Console allocation for the children Sage starts.
//!
//! Release builds of Sage use the GUI subsystem and have no console. Windows
//! gives a console program started from such a parent a new, visible console
//! window of its own, which stays blank because Sage owns the child's stdio
//! through pipes. Every background child is therefore created with
//! [`CREATION_FLAGS`]: it still gets a console, so its own console children
//! inherit that one instead of opening windows, but the console has no window.

use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

/// Added to the creation flags of every child that runs in the background.
pub const CREATION_FLAGS: u32 = CREATE_NO_WINDOW;
