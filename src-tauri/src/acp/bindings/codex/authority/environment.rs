use std::path::Path;

use crate::acp::{
    bindings::codex::{
        manifest::{MetadataValidatedPacket, Platform},
        policy::SecurityPolicyPrerequisite,
    },
    limits::LIMITS,
};

use super::VerifiedCodexConfig;

#[cfg(windows)]
#[path = "environment/windows.rs"]
mod windows;

#[cfg(all(test, windows))]
pub(crate) use windows::system_windows_directory_for_test;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum EnvironmentAuthorityError {
    #[error("Codex launch environment is unavailable on this platform")]
    PlatformUnavailable,
    #[error("Codex launch environment is not bound to the packet and configuration")]
    Binding,
    #[error(
        "Windows SystemRoot and WINDIR are not the physically verified shared system Windows directory"
    )]
    SystemRoot,
    #[error("Codex launch environment exceeds its bounded size")]
    Limit,
}

#[derive(Debug)]
pub(crate) struct CodexLaunchEnvironment {
    entries: Vec<(String, String)>,
    #[cfg(windows)]
    _windows_directory: windows::WindowsDirectoryProof,
}

impl CodexLaunchEnvironment {
    pub(crate) fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    pub(crate) fn revalidate(
        &self,
        packet: &MetadataValidatedPacket,
    ) -> Result<(), EnvironmentAuthorityError> {
        if !cfg!(windows) || packet.metadata().platform != Platform::WindowsX86_64 {
            return Err(EnvironmentAuthorityError::PlatformUnavailable);
        }
        let codex_home = packet
            .codex_home()
            .to_str()
            .ok_or(EnvironmentAuthorityError::Binding)?;
        let state_temp = format!(r"{codex_home}\tmp");
        let expected_keys = ["CODEX_HOME", "SystemRoot", "TEMP", "TMP", "WINDIR"];
        if self.entries.len() != expected_keys.len()
            || self
                .entries
                .iter()
                .zip(expected_keys)
                .any(|((key, _), expected)| key != expected)
            || self.entries[0].1 != codex_home
            || self.entries[2].1 != state_temp
            || self.entries[3].1 != state_temp
            || !self.entries[1].1.eq_ignore_ascii_case(&self.entries[4].1)
        {
            return Err(EnvironmentAuthorityError::Binding);
        }
        validate_limits(&self.entries)?;
        #[cfg(windows)]
        windows::revalidate(&self._windows_directory, &self.entries[1].1)?;
        Ok(())
    }
}

pub(crate) fn build_system_codex_launch_environment(
    packet: &MetadataValidatedPacket,
    policy: &SecurityPolicyPrerequisite,
    config: &VerifiedCodexConfig,
) -> Result<CodexLaunchEnvironment, EnvironmentAuthorityError> {
    #[cfg(windows)]
    {
        let windows = windows::system_windows_directory()?;
        build_codex_launch_environment(packet, policy, config, &windows, &windows)
    }
    #[cfg(not(windows))]
    {
        let _ = (packet, policy, config);
        Err(EnvironmentAuthorityError::PlatformUnavailable)
    }
}

pub(crate) fn build_codex_launch_environment(
    packet: &MetadataValidatedPacket,
    policy: &SecurityPolicyPrerequisite,
    config: &VerifiedCodexConfig,
    system_root: &Path,
    windir: &Path,
) -> Result<CodexLaunchEnvironment, EnvironmentAuthorityError> {
    if !cfg!(windows) || packet.metadata().platform != Platform::WindowsX86_64 {
        return Err(EnvironmentAuthorityError::PlatformUnavailable);
    }
    policy
        .revalidate(packet)
        .map_err(|_| EnvironmentAuthorityError::Binding)?;
    config
        .revalidate(policy)
        .map_err(|_| EnvironmentAuthorityError::Binding)?;
    let system_root = windows_path(system_root).ok_or(EnvironmentAuthorityError::SystemRoot)?;
    let windir = windows_path(windir).ok_or(EnvironmentAuthorityError::SystemRoot)?;
    if !system_root.eq_ignore_ascii_case(&windir) {
        return Err(EnvironmentAuthorityError::SystemRoot);
    }
    #[cfg(windows)]
    let (system_root, windows_directory) = windows::inspect(&system_root, &windir)?;
    let codex_home = policy.codex_home().to_owned();
    let state_temp = format!(r"{codex_home}\tmp");
    let entries = vec![
        ("CODEX_HOME".into(), codex_home),
        ("SystemRoot".into(), system_root.clone()),
        ("TEMP".into(), state_temp.clone()),
        ("TMP".into(), state_temp),
        ("WINDIR".into(), system_root),
    ];
    validate_limits(&entries)?;
    Ok(CodexLaunchEnvironment {
        entries,
        #[cfg(windows)]
        _windows_directory: windows_directory,
    })
}

fn windows_path(path: &Path) -> Option<String> {
    let value = path.to_str()?;
    let bytes = value.as_bytes();
    if bytes.len() < 4
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || bytes[2] != b'\\'
        || value.contains('/')
        || value.split('\\').enumerate().any(|(index, part)| {
            part.is_empty()
                || matches!(part, "." | "..")
                || (index == 0 && part.len() != 2)
                || (index > 0 && (part.contains(':') || part.ends_with(['.', ' '])))
        })
    {
        return None;
    }
    Some(value.to_owned())
}

fn validate_limits(entries: &[(String, String)]) -> Result<(), EnvironmentAuthorityError> {
    if entries.len() != 5 || entries.len() as u64 > LIMITS.process.environment_entries {
        return Err(EnvironmentAuthorityError::Limit);
    }
    let mut total = 0_u64;
    for (key, value) in entries {
        let key_bytes = key.len() as u64;
        let value_bytes = value.len() as u64;
        if key_bytes > LIMITS.process.environment_key_bytes
            || value_bytes > LIMITS.process.environment_value_bytes
        {
            return Err(EnvironmentAuthorityError::Limit);
        }
        total = total
            .checked_add(key_bytes)
            .and_then(|value| value.checked_add(value_bytes))
            .ok_or(EnvironmentAuthorityError::Limit)?;
    }
    if total > LIMITS.process.environment_bytes_total {
        return Err(EnvironmentAuthorityError::Limit);
    }
    Ok(())
}
