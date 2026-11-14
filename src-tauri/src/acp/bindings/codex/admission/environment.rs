use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use super::CodexAdmissionUnavailable;

pub(crate) trait ObservedLaunchInputs: std::fmt::Debug + Send + Sync {
    fn variable(&self, key: &str) -> Option<String>;
    fn config(&self) -> Result<Option<Vec<u8>>, CodexAdmissionUnavailable>;
    fn node_directory(&self, entrypoint: &Path) -> Option<PathBuf>;
}

#[derive(Debug)]
pub(super) struct SystemLaunchInputs;

impl ObservedLaunchInputs for SystemLaunchInputs {
    fn variable(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn config(&self) -> Result<Option<Vec<u8>>, CodexAdmissionUnavailable> {
        let home = self
            .variable("USERPROFILE")
            .map(PathBuf::from)
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
        match fs::read(home.join(".codex").join("config.toml")) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(CodexAdmissionUnavailable::IndependentAdmission),
        }
    }

    fn node_directory(&self, entrypoint: &Path) -> Option<PathBuf> {
        entrypoint
            .parent()
            .into_iter()
            .map(Path::to_path_buf)
            .chain(self.variable("PATH").into_iter().flat_map(|path| {
                path.split(';')
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .collect::<Vec<_>>()
            }))
            .find(|directory| directory.join("node.exe").is_file())
    }
}

pub(super) fn observed_environment(
    entrypoint: &Path,
    inputs: &dyn ObservedLaunchInputs,
) -> Result<Vec<(String, String)>, CodexAdmissionUnavailable> {
    #[cfg(not(windows))]
    {
        let _ = entrypoint;
        const KEYS: &[&str] = &[
            "HOME",
            "PATH",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
        ];
        KEYS.iter()
            .filter_map(|key| {
                inputs
                    .variable(key)
                    .filter(|value| !value.is_empty())
                    .map(|value| ((*key).to_owned(), value))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .try_fold(Vec::new(), |mut entries, entry| {
                if entry.0.len() > 128 || entry.1.len() > 8_192 {
                    return Err(CodexAdmissionUnavailable::IndependentAdmission);
                }
                entries.push(entry);
                Ok(entries)
            })
    }
    #[cfg(windows)]
    {
        const KEYS: &[&str] = &[
            "APPDATA",
            "ComSpec",
            "PATHEXT",
            "SystemRoot",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "WINDIR",
        ];
        let mut entries = KEYS
            .iter()
            .map(|key| {
                inputs
                    .variable(key)
                    .filter(|value| !value.is_empty())
                    .map(|value| ((*key).to_owned(), value))
                    .ok_or(CodexAdmissionUnavailable::IndependentAdmission)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let node_directory = inputs
            .node_directory(entrypoint)
            .and_then(|directory| directory.to_str().map(str::to_owned))
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
        entries.insert(0, ("PATH".into(), node_directory));
        for key in configured_provider_environment_keys(inputs)? {
            if let Some(value) = inputs.variable(&key).filter(|value| !value.is_empty()) {
                entries.push((key, value));
            }
        }
        if entries.len() > 16
            || entries
                .iter()
                .any(|(key, value)| key.len() > 128 || value.len() > 8_192)
        {
            return Err(CodexAdmissionUnavailable::IndependentAdmission);
        }
        Ok(entries)
    }
}

fn configured_provider_environment_keys(
    inputs: &dyn ObservedLaunchInputs,
) -> Result<Vec<String>, CodexAdmissionUnavailable> {
    let Some(bytes) = inputs.config()? else {
        return Ok(Vec::new());
    };
    if bytes.len() > 64 * 1024 {
        return Err(CodexAdmissionUnavailable::IndependentAdmission);
    }
    provider_environment_keys(
        std::str::from_utf8(&bytes).map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?,
    )
}

pub(super) fn provider_environment_keys(
    config: &str,
) -> Result<Vec<String>, CodexAdmissionUnavailable> {
    let config = config
        .parse::<toml::Value>()
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let provider_name = match config.get("model_provider") {
        Some(provider) => provider
            .as_str()
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?,
        None => return Ok(Vec::new()),
    };
    let providers = config
        .get("model_providers")
        .map(|providers| {
            providers
                .as_table()
                .ok_or(CodexAdmissionUnavailable::IndependentAdmission)
        })
        .transpose()?;
    let provider = match providers.and_then(|providers| providers.get(provider_name)) {
        Some(provider) => provider
            .as_table()
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?,
        None if provider_name == "openai" => return Ok(Vec::new()),
        None => return Err(CodexAdmissionUnavailable::IndependentAdmission),
    };
    let mut keys = BTreeSet::new();
    if let Some(key) = provider.get("env_key") {
        let key = key
            .as_str()
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
        insert_provider_environment_key(&mut keys, key)?;
    }
    if let Some(headers) = provider.get("env_http_headers") {
        let headers = headers
            .as_table()
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
        for key in headers.values() {
            let key = key
                .as_str()
                .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
            insert_provider_environment_key(&mut keys, key)?;
        }
    }
    Ok(keys.into_iter().collect())
}

fn insert_provider_environment_key(
    keys: &mut BTreeSet<String>,
    key: &str,
) -> Result<(), CodexAdmissionUnavailable> {
    if key.is_empty()
        || key.len() > 128
        || !key.bytes().enumerate().all(|(index, byte)| {
            byte == b'_'
                || byte.is_ascii_alphanumeric() && (index > 0 || byte.is_ascii_alphabetic())
        })
    {
        return Err(CodexAdmissionUnavailable::IndependentAdmission);
    }
    keys.insert(key.to_owned());
    Ok(())
}
