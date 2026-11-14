//! Launching Sage again brings the running copy to the front.
//!
//! The process that owns a profile listens on a doorbell named after that
//! profile. A later launch that finds the profile owned rings it and exits;
//! the owner raises its main window. Each profile has its own doorbell, so a
//! test build on a disposable profile never rings the installed app.
//!
//! The doorbell is a Windows named event in the session's `Local\` namespace,
//! created with the process token's default security: one listener thread,
//! and repeated rings coalesce into one raise. Other platforms have no
//! doorbell; a second launch there reports on stderr instead.

use std::{io, path::Path};

#[cfg(windows)]
mod named_event;

#[cfg(any(windows, test))]
#[derive(Default)]
struct RaiseGate(std::sync::Arc<std::sync::atomic::AtomicBool>);

#[cfg(any(windows, test))]
impl RaiseGate {
    fn request<E>(
        &self,
        dispatch: impl FnOnce(Box<dyn FnOnce() + Send>) -> Result<(), E>,
        raise: impl FnOnce() + Send + 'static,
    ) {
        use std::sync::{Arc, atomic::Ordering};

        if self
            .0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let pending = Arc::clone(&self.0);
        if dispatch(Box::new(move || {
            raise();
            pending.store(false, Ordering::Release);
        }))
        .is_err()
        {
            self.0.store(false, Ordering::Release);
        }
    }
}

/// Starts raising `app`'s main window whenever a later launch of this
/// profile rings. Call it once, after the main window exists, from the
/// process that owns the profile.
#[cfg(windows)]
pub(crate) fn listen(app: &tauri::AppHandle, durable_dir: &Path) -> io::Result<()> {
    let app = app.clone();
    let raises = RaiseGate::default();
    named_event::listen(&doorbell_name(durable_dir)?, move || {
        raises.request(|task| app.run_on_main_thread(task), {
            let raised = app.clone();
            move || raise(&raised)
        });
    })
}

#[cfg(not(windows))]
pub(crate) fn listen(_app: &tauri::AppHandle, _durable_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// Asks the process that owns `durable_dir` to come to the front. Fails when
/// no owner is listening.
#[cfg(windows)]
pub(crate) fn ring(durable_dir: &Path) -> io::Result<()> {
    named_event::ring(&doorbell_name(durable_dir)?)
}

#[cfg(not(windows))]
pub(crate) fn ring(_durable_dir: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(windows)]
fn raise(app: &tauri::AppHandle) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// One name per profile folder, however the path was spelled. NTFS folds
/// case, so the Windows key does too.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn doorbell_name(durable_dir: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};

    let canonical = durable_dir.canonicalize()?;
    let key = canonical.to_string_lossy();
    let key = if cfg!(windows) {
        key.to_lowercase()
    } else {
        key.into_owned()
    };
    let digest = Sha256::digest(key.as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!(r"Local\ai.sage.desktop.profile.{hex}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doorbell_bursts_queue_one_raise_until_the_dispatcher_resumes() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let gate = RaiseGate::default();
        let raises = Arc::new(AtomicUsize::new(0));
        let mut queue = std::collections::VecDeque::new();
        for _ in 0..10_000 {
            let raises = Arc::clone(&raises);
            gate.request(
                |task| {
                    queue.push_back(task);
                    Ok::<_, ()>(())
                },
                move || {
                    raises.fetch_add(1, Ordering::Relaxed);
                },
            );
        }
        assert_eq!(queue.len(), 1);
        assert_eq!(raises.load(Ordering::Relaxed), 0);
        queue.pop_front().unwrap()();
        assert_eq!(raises.load(Ordering::Relaxed), 1);

        let raises_again = Arc::clone(&raises);
        gate.request(
            |task| {
                queue.push_back(task);
                Ok::<_, ()>(())
            },
            move || {
                raises_again.fetch_add(1, Ordering::Relaxed);
            },
        );
        assert_eq!(queue.len(), 1);
        queue.pop_front().unwrap()();
        assert_eq!(raises.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_failed_dispatch_does_not_block_the_next_raise() {
        let gate = RaiseGate::default();
        gate.request(|_| Err(()), || panic!("failed dispatch must not execute"));
        let (raised, raises) = std::sync::mpsc::sync_channel(1);
        gate.request(
            |task| {
                task();
                Ok::<_, ()>(())
            },
            move || raised.send(()).unwrap(),
        );
        assert_eq!(raises.try_recv(), Ok(()));
    }

    #[test]
    fn each_profile_folder_has_one_session_scoped_doorbell() {
        let root = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-profile-handoff-{}", uuid::Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();

        let name = doorbell_name(&first).unwrap();
        let respelled = doorbell_name(&first.join("..").join("first")).unwrap();
        let other = doorbell_name(&second).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        assert!(name.starts_with(r"Local\"));
        assert_eq!(name, respelled);
        assert_ne!(name, other);
    }
}
