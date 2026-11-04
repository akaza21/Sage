use std::{ffi::OsString, os::windows::ffi::OsStrExt};

use super::TerminalError;

/// Makes `cmd.exe` resolve typed commands from `PATH` only, so a program or
/// script planted in the project folder never shadows a real tool.
const NO_CURRENT_DIRECTORY_COMMAND_SEARCH: (&str, &str) =
    ("NoDefaultCurrentDirectoryInExePath", "1");
/// The standard user variables `cmd.exe` receives when Sage has them, since
/// `cmd.exe` reads no profile to rebuild them. The list is closed: credentials,
/// tool settings and Sage's own variables stay out. It leaves out the variable
/// above, so a host value can never override that one.
#[rustfmt::skip]
const PASSED_VARIABLES: [&str; 29] = [
    "ComSpec", "PATH", "PATHEXT", "PSModulePath", "TEMP", "TMP",
    "SystemDrive", "SystemRoot", "windir", "OS", "COMPUTERNAME",
    "PROCESSOR_ARCHITECTURE", "NUMBER_OF_PROCESSORS",
    "USERPROFILE", "USERNAME", "USERDOMAIN", "HOMEDRIVE", "HOMEPATH",
    "APPDATA", "LOCALAPPDATA", "ProgramData", "ALLUSERSPROFILE", "PUBLIC",
    "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432",
    "CommonProgramFiles", "CommonProgramFiles(x86)", "CommonProgramW6432",
];
/// Windows's limit for one variable's value, in UTF-16 units.
const VARIABLE_UNITS: usize = 32_767;
/// Room for a `PATH` at `VARIABLE_UNITS` beside the other listed variables.
const BLOCK_UNITS: usize = 65_536;

pub(super) fn environment_block() -> Result<Vec<u16>, TerminalError> {
    environment_block_from(std::env::var_os)
}

/// Builds the block from `host`, which is Sage's own environment outside
/// tests, looking up one listed name at a time. Values are copied as UTF-16
/// unchanged: a value with a NUL or over the variable limit, or one that would
/// take the block over its cap, fails the launch before it is copied.
pub(super) fn environment_block_from(
    host: impl Fn(&'static str) -> Option<OsString>,
) -> Result<Vec<u16>, TerminalError> {
    let (search, disabled) = NO_CURRENT_DIRECTORY_COMMAND_SEARCH;
    let mut names = Vec::with_capacity(PASSED_VARIABLES.len() + 1);
    names.push(search);
    names.extend(PASSED_VARIABLES);
    // Windows asks for entries sorted by name ignoring case, compared as
    // uppercase ordinals (the order Rust's own `Command` uses).
    names.sort_by_cached_key(|name| name.to_ascii_uppercase());
    let mut block = Vec::new();
    for name in names {
        let value = if name == search {
            OsString::from(disabled)
        } else if let Some(value) = host(name) {
            value
        } else {
            continue;
        };
        // Each value and the block budget are checked before anything is
        // copied, counting `=`, the entry's NUL and the block's final NUL.
        let units = value.encode_wide().count();
        let needed = name.len() + 1 + units + 1;
        if units > VARIABLE_UNITS
            || block.len() + needed + 1 > BLOCK_UNITS
            || value.encode_wide().any(|unit| unit == 0)
        {
            return Err(TerminalError::Backend);
        }
        block.reserve(needed);
        block.extend(name.encode_utf16().chain([u16::from(b'=')]));
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    Ok(block)
}
