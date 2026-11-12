//! Startup refusals a person must read: Sage stops before opening the
//! profile's databases and says why in plain language.
//!
//! The refusal is always returned as the setup error, which `run` prints to
//! stderr. The everyday profile also gets a native message box, because the
//! installed app has no console. Disposable profiles are launched by tooling
//! that reads stderr and must not block on a modal dialog.

use crate::launch_profile::{LaunchProfile, PROFILE_MARKER_BYTES, PROFILE_MARKER_NAME};

pub(crate) type SetupError = Box<dyn std::error::Error>;

/// The text for a launch that found its profile owned by another process.
pub(crate) fn profile_in_use(profile: &LaunchProfile) -> String {
    match profile.disposable_root() {
        None => {
            let marker = String::from_utf8_lossy(PROFILE_MARKER_BYTES);
            format!(
                "Sage is already running.\n\n\
                 Only one copy of Sage can use your Sage data at a time, so this copy \
                 will close without opening or changing anything. Switch to the Sage \
                 window that is already open.\n\n\
                 To run a test or development build next to it, give that build its \
                 own profile: make an empty folder containing a file named \
                 {PROFILE_MARKER_NAME} whose only line is {}, then start the build with \
                 SAGE_DISPOSABLE_PROFILE_ROOT set to that folder.",
                marker.trim_end()
            )
        }
        Some(root) => format!(
            "Another Sage process is already using the test profile at {}. This copy \
             will close without opening or changing anything. Give each test build \
             its own SAGE_DISPOSABLE_PROFILE_ROOT folder.",
            root.display()
        ),
    }
}

/// Shows `message` where the person launching `profile` will see it and
/// returns it as the setup error that stops Sage.
pub(crate) fn refuse(profile: &LaunchProfile, message: String) -> SetupError {
    if !profile.is_disposable() {
        show_native(&message);
    }
    message.into()
}

#[cfg(windows)]
fn show_native(message: &str) {
    use windows::{
        Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MessageBoxW},
        core::HSTRING,
    };

    // Setup runs on the main thread before the event loop starts, so this
    // modal box owns its own message loop and returns when dismissed.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from("Sage"),
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND,
        );
    }
}

#[cfg(not(windows))]
fn show_native(_message: &str) {}
