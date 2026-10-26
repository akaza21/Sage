//! Opens a PTY pair whose descriptors are close-on-exec from the start, so a
//! process another Sage thread spawns meanwhile never inherits either end.

use std::{
    ffi::{CStr, CString},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};

use super::super::TerminalError;

/// The master is non-blocking: Sage's reader and writer wait in `poll`, so
/// neither can block past a close.
pub(super) fn open(columns: u16, rows: u16) -> Result<(OwnedFd, OwnedFd), TerminalError> {
    let master = open_terminal(c"/dev/ptmx")?;
    // SAFETY: grantpt and unlockpt only act on the master opened above.
    if unsafe { libc::grantpt(master.as_raw_fd()) } < 0
        || unsafe { libc::unlockpt(master.as_raw_fd()) } < 0
    {
        return Err(TerminalError::Backend);
    }
    let slave = open_terminal(&slave_name(&master)?)?;
    resize(&master, columns, rows)?;
    // SAFETY: F_GETFL and F_SETFL only change the status flags of our master.
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(TerminalError::Backend);
    }
    Ok((master, slave))
}

pub(super) fn resize(master: &OwnedFd, columns: u16, rows: u16) -> Result<(), TerminalError> {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCSWINSZ reads one winsize from a valid pointer.
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) } < 0 {
        Err(TerminalError::Backend)
    } else {
        Ok(())
    }
}

/// O_NOCTTY keeps the terminal from becoming Sage's own controlling terminal.
fn open_terminal(path: &CStr) -> Result<OwnedFd, TerminalError> {
    // SAFETY: the path is NUL-terminated and outlives the call.
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(TerminalError::Backend);
    }
    // SAFETY: open returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
fn slave_name(master: &OwnedFd) -> Result<CString, TerminalError> {
    let mut name = [0 as libc::c_char; 128];
    // SAFETY: ptsname_r writes at most `name.len()` bytes, NUL included.
    if unsafe { libc::ptsname_r(master.as_raw_fd(), name.as_mut_ptr(), name.len()) } != 0 {
        return Err(TerminalError::Backend);
    }
    // SAFETY: on success the buffer holds a NUL-terminated path.
    Ok(unsafe { CStr::from_ptr(name.as_ptr()) }.to_owned())
}

/// macOS has no ptsname_r before 10.13.4; TIOCPTYGNAME is what its ptsname
/// uses, into a buffer of the documented 128 bytes.
#[cfg(target_os = "macos")]
fn slave_name(master: &OwnedFd) -> Result<CString, TerminalError> {
    let mut name = [0 as libc::c_char; 128];
    // SAFETY: TIOCPTYGNAME writes a NUL-terminated path of at most 128 bytes.
    if unsafe {
        libc::ioctl(
            master.as_raw_fd(),
            libc::TIOCPTYGNAME as _,
            name.as_mut_ptr(),
        )
    } < 0
    {
        return Err(TerminalError::Backend);
    }
    // SAFETY: on success the buffer holds a NUL-terminated path.
    Ok(unsafe { CStr::from_ptr(name.as_ptr()) }.to_owned())
}
