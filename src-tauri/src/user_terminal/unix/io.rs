//! Moves bytes between the PTY master and Sage. The master is non-blocking and
//! both loops wait in `poll` with a short timeout, so neither thread blocks past
//! a close, even when a program stops reading its input or a process that left
//! the session keeps the terminal open.

use std::{
    fs::File,
    io::{ErrorKind, Read, Write},
    os::fd::{AsRawFd, OwnedFd},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
};

use super::super::{OUTPUT_CHUNK_BYTES, TerminalOutputSink};

const POLL_MS: libc::c_int = 100;

/// Whether `file` became ready for `events` within one poll interval.
fn ready(file: &File, events: libc::c_short) -> std::io::Result<bool> {
    let mut poll = libc::pollfd {
        fd: file.as_raw_fd(),
        events,
        revents: 0,
    };
    // SAFETY: one valid pollfd for a descriptor the caller owns.
    let ready = unsafe { libc::poll(&mut poll, 1, POLL_MS) };
    if ready < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == ErrorKind::Interrupted {
            Ok(false)
        } else {
            Err(error)
        };
    }
    // An invalid or failed descriptor would report ready forever.
    if ready > 0 && poll.revents & (libc::POLLNVAL | libc::POLLERR) != 0 {
        return Err(std::io::Error::other("terminal descriptor failed"));
    }
    Ok(ready > 0)
}

/// Reads the PTY until the shell's session closes it or close is requested.
pub(super) fn drain(master: OwnedFd, output: &TerminalOutputSink, stop: &AtomicBool) {
    let mut file = File::from(master);
    let mut chunk = [0_u8; OUTPUT_CHUNK_BYTES];
    while !stop.load(Ordering::Acquire) {
        match ready(&file, libc::POLLIN) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => break,
        }
        match file.read(&mut chunk) {
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
            // Linux reports EIO once the last slave descriptor closes.
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if output.append(&chunk[..count]).is_err() {
                    break;
                }
            }
        }
    }
}

/// Writes queued input in order until close drops the queue's sender. A
/// program that is not reading only fills the queue; Sage's locks are never
/// held while the PTY is full.
pub(super) fn feed(master: OwnedFd, input: &Receiver<Vec<u8>>, stop: &AtomicBool) {
    let mut file = File::from(master);
    while let Ok(bytes) = input.recv() {
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            if stop.load(Ordering::Acquire) {
                return;
            }
            match file.write(rest) {
                Ok(0) => return,
                Ok(count) => rest = &rest[count..],
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if ready(&file, libc::POLLOUT).is_err() {
                        return;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    }
}
