//! An owned Windows kernel handle.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};

use crate::error::LaunchError;

pub struct NativeHandle(HANDLE);

// Windows kernel handles may be waited, queried, and closed from any thread;
// this wrapper owns exactly one handle and closes it once on drop.
unsafe impl Send for NativeHandle {}
unsafe impl Sync for NativeHandle {}

impl NativeHandle {
    /// Takes ownership of `value`, which is closed when the wrapper drops.
    ///
    /// # Safety
    ///
    /// `value` must be null, `INVALID_HANDLE_VALUE`, or a kernel handle the
    /// caller owns and transfers here; nothing else may close it afterwards.
    pub unsafe fn new(value: HANDLE) -> Result<Self, LaunchError> {
        if value.is_null() || value == INVALID_HANDLE_VALUE {
            Err(LaunchError::Io)
        } else {
            Ok(Self(value))
        }
    }

    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for NativeHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the wrapper uniquely owns this valid kernel handle.
            unsafe { CloseHandle(self.0) };
            self.0 = null_mut();
        }
    }
}
