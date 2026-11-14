//! Executable fixtures must not leave writable descriptors in sibling children.

use std::{io, path::Path};

/// Write a script, preserving the caller's choice of executable permissions.
/// Callers own the fixture path; it must not be written again during execution.
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::{
            ffi::OsStr,
            os::unix::ffi::OsStrExt,
            process::{Command, Stdio},
        };

        // Write in a child, so another test's fork cannot inherit a writable
        // descriptor to this script. CLOEXEC and closing a writer in the test
        // process are insufficient: an already-forked child may still hold it
        // until exec, making an immediate probe fail with ETXTBSY.
        // https://github.com/rust-lang/rust/issues/114554
        // Both path and script bytes are quoted positional arguments, never
        // shell source. printf is a shell builtin; wait for the writer to exit.
        let status = Command::new("/bin/sh")
            .args(["-c", "printf '%s' \"$2\" > \"$1\"", "sage-test-executable"])
            .arg(path.as_ref())
            .arg(OsStr::from_bytes(contents.as_ref()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "script writer exited with {status}"
            )))
        }
    }
    #[cfg(not(unix))]
    {
        // std creates non-inheritable Windows handles and closes the writer
        // before returning. There is no Unix fork window to drain here.
        std::fs::write(path, contents)
    }
}
