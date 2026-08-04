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
}
