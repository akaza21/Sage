//! Launches that must stop before opening the profile's databases.
//!
//! Tauri runs setup inside the event loop and panics on a setup error, so a
//! refusal never returns one: it says what happened and ends the process with
//! an ordinary exit. Nothing has been opened yet, so there is nothing to
//! close. The person at the desktop gets at most one short sentence; the
//! detail and any developer guidance go to stderr. Disposable profiles are
//! launched by tooling that reads stderr, so they never get a modal dialog.

use std::{io, path::Path};

use crate::launch_profile::{LaunchProfile, PROFILE_MARKER_BYTES, PROFILE_MARKER_NAME};

#[derive(Debug)]
pub(crate) enum Refusal {
    /// Another live process owns the profile.
    ProfileInUse,
    /// A newer Sage migrated the profile; the text is the error's detail.
    NewerProfile(String),
}

/// How a refused launch ends.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ending {
    pub(crate) code: i32,
    pub(crate) dialog: Option<&'static str>,
    pub(crate) stderr: String,
}

const ALREADY_OPEN: &str = "Sage is already open.";
const NEWER_PROFILE: &str = "This Sage data was saved by a newer version of Sage. Install the \
     latest Sage to open it. Your data has not been changed.";

/// Decides how a refused launch of `profile` ends. `hand_off` asks the
/// profile's owner to come to the front; it is tried only for the everyday
/// profile, where a second launch means "show me Sage".
pub(crate) fn ending(
    refusal: &Refusal,
    profile: &LaunchProfile,
    hand_off: impl FnOnce(&Path) -> io::Result<()>,
) -> Ending {
    let durable_dir = profile.durable_dir();
    match (refusal, profile.disposable_root()) {
        (Refusal::ProfileInUse, Some(root)) => Ending {
            code: 1,
            dialog: None,
            stderr: format!(
                "Another Sage process is already using the test profile at {}. Give each \
                 test build its own SAGE_DISPOSABLE_PROFILE_ROOT folder.",
                root.display()
            ),
        },
        (Refusal::ProfileInUse, None) => {
            let (code, dialog, outcome) = match hand_off(durable_dir) {
                Ok(()) => (0, None, "brought it to the front".to_owned()),
                Err(error) => (
                    1,
                    Some(ALREADY_OPEN),
                    format!("could not reach it ({error})"),
                ),
            };
            Ending {
                code,
                dialog,
                stderr: format!(
                    "Sage is already running with the profile at {} and this launch {outcome}. \
                     {}",
                    durable_dir.display(),
                    own_profile_hint()
                ),
            }
        }
        (Refusal::NewerProfile(detail), root) => Ending {
            code: 1,
            dialog: root.is_none().then_some(NEWER_PROFILE),
            stderr: format!("{detail} {}", own_profile_hint()),
        },
    }
}

/// Reports `refusal` and exits the process.
pub(crate) fn exit(refusal: Refusal, profile: &LaunchProfile) -> ! {
    let ending = ending(&refusal, profile, crate::profile_handoff::ring);
    eprintln!("{}", ending.stderr);
    if let Some(text) = ending.dialog {
        show_native(text);
    }
    std::process::exit(ending.code)
}

fn own_profile_hint() -> String {
    let marker = String::from_utf8_lossy(PROFILE_MARKER_BYTES);
    format!(
        "To run a test or development build beside the installed Sage, give it its own \
         profile: an empty folder containing a file named {PROFILE_MARKER_NAME} whose only \
         line is {}, passed as SAGE_DISPOSABLE_PROFILE_ROOT.",
        marker.trim_end()
    )
}

#[cfg(windows)]
fn show_native(message: &str) {
    use windows::{
        Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MessageBoxW},
        core::HSTRING,
    };

    // A modal box runs its own message loop and returns when dismissed.
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn normal() -> LaunchProfile {
        LaunchProfile::Normal {
            durable_dir: PathBuf::from("/profiles/sage"),
        }
    }

    fn disposable() -> LaunchProfile {
        LaunchProfile::Disposable {
            durable_dir: PathBuf::from("/lane/profile/durable"),
            webview_dir: PathBuf::from("/lane/profile/webview"),
            project_root: None,
        }
    }

    fn unreachable_owner(_: &Path) -> io::Result<()> {
        Err(io::ErrorKind::NotFound.into())
    }

    #[test]
    fn a_second_everyday_launch_hands_off_silently_and_falls_back_to_one_sentence() {
        let handed_off = ending(&Refusal::ProfileInUse, &normal(), |directory| {
            assert_eq!(directory, Path::new("/profiles/sage"));
            Ok(())
        });
        assert_eq!((handed_off.code, handed_off.dialog), (0, None));

        let unreachable = ending(&Refusal::ProfileInUse, &normal(), unreachable_owner);
        assert_eq!(unreachable.code, 1);
        assert_eq!(unreachable.dialog, Some("Sage is already open."));
        assert!(unreachable.stderr.contains("SAGE_DISPOSABLE_PROFILE_ROOT"));
    }

    #[test]
    fn a_test_profile_in_use_reports_on_stderr_without_ringing_or_a_dialog() {
        let ending = ending(&Refusal::ProfileInUse, &disposable(), |_| {
            panic!("a disposable profile must not ring its owner")
        });
        assert_eq!((ending.code, ending.dialog), (1, None));
        assert!(ending.stderr.contains("/lane/profile"));
    }

    #[test]
    fn a_newer_profile_tells_the_person_to_update_and_developers_the_rest() {
        let refusal = Refusal::NewerProfile("data version 99".into());
        let everyday = ending(&refusal, &normal(), unreachable_owner);
        assert_eq!(everyday.code, 1);
        let dialog = everyday.dialog.expect("the everyday profile gets a dialog");
        assert!(!dialog.contains("SAGE_") && !dialog.contains(PROFILE_MARKER_NAME));
        assert!(everyday.stderr.contains("data version 99"));
        assert!(everyday.stderr.contains("SAGE_DISPOSABLE_PROFILE_ROOT"));

        assert_eq!(
            ending(&refusal, &disposable(), unreachable_owner).dialog,
            None
        );
    }
}
