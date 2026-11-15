use std::ffi::OsString;

use super::profile::StandardAcpProfile;

#[cfg(windows)]
pub(crate) const WINDOWS_PROCESS_BASELINE_KEYS: [&str; 10] = [
    "APPDATA",
    "ComSpec",
    "LOCALAPPDATA",
    "PATH",
    "PATHEXT",
    "SystemRoot",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "WINDIR",
];

pub(crate) fn launch_environment(
    profile: &StandardAcpProfile,
    read: impl Fn(&str) -> Option<OsString>,
) -> Option<Vec<(OsString, OsString)>> {
    let mut entries = baseline(&read)?;
    for (key, value) in passthrough(profile, &read) {
        if !contains_key(&entries, &key) {
            entries.push((key, value));
        }
    }
    Some(entries)
}

/// Only names are configured; values are read at launch and never stored.
pub(crate) fn extend_passthrough(
    entries: &mut Vec<(OsString, OsString)>,
    names: &[String],
    read: impl Fn(&str) -> Option<OsString>,
) {
    for name in names {
        let key = OsString::from(name);
        if contains_key(entries, &key) {
            continue;
        }
        if let Some(value) = read(name).filter(|value| !value.is_empty()) {
            entries.push((key, value));
        }
    }
}

#[cfg(windows)]
fn baseline(read: &impl Fn(&str) -> Option<OsString>) -> Option<Vec<(OsString, OsString)>> {
    let entries: Vec<(OsString, OsString)> = WINDOWS_PROCESS_BASELINE_KEYS
        .iter()
        .filter_map(|key| {
            let value = read(key).filter(|value| !value.is_empty())?;
            Some((OsString::from(*key), value))
        })
        .collect();
    contains_key(&entries, std::ffi::OsStr::new("SystemRoot")).then_some(entries)
}

#[cfg(not(windows))]
fn baseline(read: &impl Fn(&str) -> Option<OsString>) -> Option<Vec<(OsString, OsString)>> {
    let _ = read;
    Some(Vec::new())
}

fn contains_key(entries: &[(OsString, OsString)], key: &std::ffi::OsStr) -> bool {
    entries.iter().any(|(present, _)| {
        present
            .to_string_lossy()
            .eq_ignore_ascii_case(key.to_string_lossy().as_ref())
    })
}

pub(crate) fn passthrough(
    profile: &StandardAcpProfile,
    read: impl Fn(&str) -> Option<OsString>,
) -> Vec<(OsString, OsString)> {
    profile
        .passthrough_env
        .iter()
        .filter_map(|key| {
            let value = read(key).filter(|value| !value.is_empty())?;
            Some((OsString::from(*key), value))
        })
        .collect()
}
