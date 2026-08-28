use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use crate::acp::limits::LIMITS;

use super::super::supervisor::LaunchError;
use super::MAX_WINDOWS_WIDE;

pub(super) fn command_line(executable: &Path, args: &[OsString]) -> Result<Vec<u16>, LaunchError> {
    let mut result = Vec::new();
    append_quoted(&mut result, executable.as_os_str());
    for argument in args {
        result.push(b' ' as u16);
        append_quoted(&mut result, argument);
    }
    result.push(0);
    if result.len() > MAX_WINDOWS_WIDE {
        return Err(LaunchError::ArgumentLimit);
    }
    Ok(result)
}

pub(super) fn batch_command_line(
    launcher: &Path,
    args: &[OsString],
) -> Result<String, LaunchError> {
    let mut command = String::new();
    append_batch_quoted(&mut command, &cmd_path(launcher)?)?;
    for argument in args {
        command.push(' ');
        append_batch_quoted(&mut command, argument)?;
    }
    if command.encode_utf16().count() >= MAX_WINDOWS_WIDE {
        return Err(LaunchError::ArgumentLimit);
    }
    Ok(command)
}

pub(super) fn cmd_batch_launcher_command_line(
    cmd: &Path,
    launcher: &Path,
    args: &[OsString],
) -> Result<Vec<u16>, LaunchError> {
    let command = batch_command_line(launcher, args)?;
    let mut result = Vec::new();
    append_quoted(&mut result, cmd.as_os_str());
    result.extend(" /d /s /c \"".encode_utf16());
    result.extend(command.encode_utf16());
    result.push(b'"' as u16);
    result.push(0);
    if result.len() > MAX_WINDOWS_WIDE {
        return Err(LaunchError::ArgumentLimit);
    }
    Ok(result)
}

fn cmd_path(path: &Path) -> Result<OsString, LaunchError> {
    let path = path.to_str().ok_or(LaunchError::InvalidArgument)?;
    let path = path
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!(r"\\{path}"))
        .or_else(|| path.strip_prefix(r"\\?\").map(str::to_owned))
        .unwrap_or_else(|| path.to_owned());
    if path.starts_with(r"\\.\") {
        return Err(LaunchError::InvalidArgument);
    }
    Ok(OsString::from(path))
}

/// `cmd.exe` treats both ordinary and verbatim UNC working directories as
/// unsupported and silently falls back to the Windows directory. A local
/// verbatim path is safe once converted to its normal DOS spelling.
pub(super) fn cmd_working_directory(path: &Path) -> Result<OsString, LaunchError> {
    let path = path.to_str().ok_or(LaunchError::InvalidAbsolutePath)?;
    if let Some(path) = path.strip_prefix(r"\\?\") {
        if path.starts_with("UNC\\") {
            return Err(LaunchError::InvalidAbsolutePath);
        }
        return local_dos_directory(path);
    }
    local_dos_directory(path)
}

fn local_dos_directory(path: &str) -> Result<OsString, LaunchError> {
    let bytes = path.as_bytes();
    if path.starts_with(r"\\")
        || bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
    {
        return Err(LaunchError::InvalidAbsolutePath);
    }
    Ok(OsString::from(path))
}

fn append_batch_quoted(target: &mut String, value: &OsStr) -> Result<(), LaunchError> {
    let value = value.to_str().ok_or(LaunchError::InvalidArgument)?;
    if value.is_empty()
        || value.contains([
            '\0', '\r', '\n', '"', '%', '!', '^', '&', '|', '<', '>', '(', ')',
        ])
    {
        return Err(LaunchError::InvalidArgument);
    }
    target.push('"');
    target.push_str(value);
    target.push('"');
    Ok(())
}

fn append_quoted(target: &mut Vec<u16>, value: &OsStr) {
    let units: Vec<u16> = value.encode_wide().collect();
    let quote = units.is_empty() || units.iter().any(|unit| matches!(*unit, 9 | 32 | 34));
    if !quote {
        target.extend(units);
        return;
    }
    target.push(b'"' as u16);
    let mut slashes = 0_usize;
    for unit in units {
        if unit == b'\\' as u16 {
            slashes += 1;
        } else {
            if unit == b'"' as u16 {
                target.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
            } else {
                target.extend(std::iter::repeat_n(b'\\' as u16, slashes));
            }
            slashes = 0;
            target.push(unit);
        }
    }
    target.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    target.push(b'"' as u16);
}

pub(super) fn environment_block(values: &[(OsString, OsString)]) -> Result<Vec<u16>, LaunchError> {
    let mut sorted: Vec<_> = values.iter().collect();
    sorted.sort_by_key(|(key, _)| key.to_string_lossy().to_lowercase());
    let mut result = Vec::new();
    for (key, value) in sorted {
        result.extend(key.encode_wide());
        result.push(b'=' as u16);
        result.extend(value.encode_wide());
        result.push(0);
    }
    result.push(0);
    if values.is_empty() {
        result.push(0);
    }
    if result.len().saturating_mul(2) > LIMITS.process.environment_bytes_total as usize {
        return Err(LaunchError::EnvironmentLimit);
    }
    Ok(result)
}

pub(super) fn wide_z(value: &OsStr) -> Result<Vec<u16>, LaunchError> {
    let mut result: Vec<u16> = value.encode_wide().collect();
    if result.contains(&0) || result.len() >= MAX_WINDOWS_WIDE {
        return Err(LaunchError::InvalidAbsolutePath);
    }
    result.push(0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_empty_quotes_and_trailing_slashes() {
        let mut quoted = Vec::new();
        append_quoted(&mut quoted, OsStr::new(""));
        assert_eq!(String::from_utf16(&quoted).unwrap(), "\"\"");
        quoted.clear();
        append_quoted(&mut quoted, OsStr::new("a b\\"));
        assert_eq!(String::from_utf16(&quoted).unwrap(), "\"a b\\\\\"");
        quoted.clear();
        append_quoted(&mut quoted, OsStr::new("a\\\"b"));
        assert_eq!(String::from_utf16(&quoted).unwrap(), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn environment_is_sorted_and_double_nul_terminated() {
        let values = vec![
            (OsString::from("z"), OsString::from("2")),
            (OsString::from("A"), OsString::from("1")),
        ];
        assert_eq!(
            String::from_utf16_lossy(&environment_block(&values).unwrap()),
            "A=1\0z=2\0\0"
        );
        assert_eq!(environment_block(&[]).unwrap(), [0, 0]);
    }

    #[test]
    fn batch_command_quotes_the_observed_launcher_and_rejects_shell_metacharacters() {
        assert_eq!(
            batch_command_line(
                Path::new(r"C:\Users\name\AppData\Roaming\npm\codex.cmd"),
                &[OsString::from("app-server")]
            )
            .unwrap(),
            r#""C:\Users\name\AppData\Roaming\npm\codex.cmd" "app-server""#
        );
        assert_eq!(
            batch_command_line(Path::new(r"C:\bad&name\codex.cmd"), &[]).err(),
            Some(LaunchError::InvalidArgument)
        );
    }

    #[test]
    fn cmd_batch_launcher_command_keeps_the_launcher_as_one_quoted_command() {
        let command = cmd_batch_launcher_command_line(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            Path::new(r"C:\Users\name\AppData\Roaming\npm\codex.cmd"),
            &[OsString::from("app-server")],
        )
        .unwrap();
        assert_eq!(
            String::from_utf16(&command[..command.len() - 1]).unwrap(),
            r##"C:\Windows\System32\cmd.exe /d /s /c ""C:\Users\name\AppData\Roaming\npm\codex.cmd" "app-server"""##
        );
    }

    #[test]
    fn batch_command_converts_verbatim_launcher_paths_for_cmd() {
        assert_eq!(
            batch_command_line(
                Path::new(r"\\?\C:\Users\name\AppData\Roaming\npm\codex.cmd"),
                &[]
            )
            .unwrap(),
            r#""C:\Users\name\AppData\Roaming\npm\codex.cmd""#
        );
    }

    #[test]
    fn cmd_working_directory_converts_local_verbatim_paths_and_rejects_unc_paths() {
        assert_eq!(
            cmd_working_directory(Path::new(r"\\?\C:\Users\name\Sage")).unwrap(),
            OsString::from(r"C:\Users\name\Sage")
        );
        assert_eq!(
            cmd_working_directory(Path::new(r"\\?\UNC\server\share\Sage")),
            Err(LaunchError::InvalidAbsolutePath)
        );
        assert_eq!(
            cmd_working_directory(Path::new(r"\\server\share\Sage")),
            Err(LaunchError::InvalidAbsolutePath)
        );
    }
}
