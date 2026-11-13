//! The doorbell as a Windows auto-reset named event.

use std::{io, os::windows::ffi::OsStrExt};

use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::{CreateEventW, OpenEventW, SetEvent, WaitForSingleObject},
};

const EVENT_MODIFY_STATE: u32 = 0x0002;
const INFINITE: u32 = u32::MAX;
const WAIT_OBJECT_0: u32 = 0;

struct Event(HANDLE);

// The handle is a kernel object reference; the waiting thread is its only
// user once it is moved there.
unsafe impl Send for Event {}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(name: &str) -> Vec<u16> {
    std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn checked(handle: HANDLE) -> io::Result<Event> {
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(Event(handle))
    }
}

pub(super) fn listen(name: &str, on_ring: impl Fn() + Send + 'static) -> io::Result<()> {
    let name = wide(name);
    // Null attributes: the token's default security and a handle no child
    // process inherits. Auto-reset, so a ring wakes the waiter once.
    let event = checked(unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) })?;
    // The owner creates the doorbell after taking the profile lock, so an
    // existing one belongs to someone else; sharing it would let them decide
    // when this Sage is raised.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "another process already created this profile's doorbell",
        ));
    }
    std::thread::Builder::new()
        .name("sage-profile-doorbell".into())
        .spawn(move || {
            let event = event;
            while unsafe { WaitForSingleObject(event.0, INFINITE) } == WAIT_OBJECT_0 {
                on_ring();
            }
        })?;
    Ok(())
}

pub(super) fn ring(name: &str) -> io::Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

    let name = wide(name);
    let event = checked(unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) })?;
    // This launch holds the foreground; lend it to the owner it is ringing.
    let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    if unsafe { SetEvent(event.0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::*;

    #[test]
    fn a_ring_reaches_a_listening_owner_and_fails_without_one() {
        let name = format!(r"Local\ai.sage.desktop.test.{}", uuid::Uuid::new_v4());
        assert!(ring(&name).is_err());

        let (rang, rings) = mpsc::sync_channel(1);
        listen(&name, move || {
            let _ = rang.try_send(());
        })
        .unwrap();
        ring(&name).unwrap();
        rings.recv_timeout(Duration::from_secs(10)).unwrap();

        assert_eq!(
            listen(&name, || {}).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }
}
