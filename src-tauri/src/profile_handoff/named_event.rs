//! The doorbell as a Windows auto-reset named event.

use std::{io, os::windows::ffi::OsStrExt};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
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
