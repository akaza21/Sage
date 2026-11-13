//! Ignore configuration read by the same admitted, environment-cleared Git
//! command as status. Steady notices only stat bounded source fingerprints.

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::{admit, cache, probe_git};

const MAX_SOURCES: usize = 64;
const MAX_SOURCE_PATH_BYTES: usize = 64 * 1024;

pub(crate) struct IgnoreConfig {
    pub(crate) excludes: Option<PathBuf>,
    sources: GitRuleSources,
}

impl IgnoreConfig {
    pub(crate) async fn load(root: &Path) -> Option<Self> {
        let repository = admit(root, None).await.ok()??;
        let slot = cache::admission_slot(root);
        let cached = slot.lock().await;
        let admission = cached.as_ref()?;
        if !admission.is_current() || admission.repository.as_ref() != Some(&repository) {
            return None;
        }
        let mut sources = GitRuleSources::default();
        for (path, _) in &admission.watched {
            sources.note(path);
        }
        drop(cached);
        let output = probe_git(
            root,
            None,
            Some(&repository),
            &["config", "--path", "-z", "--get", "core.excludesFile"],
            MAX_SOURCE_PATH_BYTES,
        )
        .await
        .ok()?;
        if output.stdout_truncated || !sources.is_current() {
            return None;
        }
        let excludes = if output.status.success() {
            let bytes = output.stdout.strip_suffix(&[0])?;
            let text = std::str::from_utf8(bytes).ok()?;
            if text.contains('\0') {
                return None;
            }
            // Git resolves relative core.excludesFile values from its cwd,
            // the opened root, rather than from the config file's folder.
            (!text.is_empty()).then(|| root.join(text))
        } else if output.status.code() == Some(1) && output.stdout.is_empty() {
            // The runner clears HOME and XDG_CONFIG_HOME as well as disabling
            // global/system config, so there is no user-default excludes file.
            None
        } else {
            return None;
        };
        Some(Self { excludes, sources })
    }

    pub(crate) fn is_current(&self) -> bool {
        self.sources.is_current()
    }
}

/// A small set of resolved rule/config sources, including absent files so
/// later creation invalidates a decision. Uninspectable or over-budget sets
/// can never establish freshness. No file contents survive a read.
#[derive(Default)]
pub(crate) struct GitRuleSources {
    files: Vec<(PathBuf, Option<Stamp>)>,
    path_bytes: usize,
    unknown: bool,
}

impl GitRuleSources {
    pub(crate) fn note(&mut self, path: &Path) {
        if self.files.iter().any(|(known, _)| known == path) {
            return;
        }
        if self.files.len() >= MAX_SOURCES
            || self.path_bytes + path.as_os_str().len() > MAX_SOURCE_PATH_BYTES
        {
            self.unknown = true;
            return;
        }
        self.path_bytes += path.as_os_str().len();
        self.files.push((path.to_path_buf(), Stamp::of(path)));
    }

    pub(crate) fn is_current(&self) -> bool {
        !self.unknown
            && self.files.iter().all(|(path, before)| {
                before.is_some() && Stamp::of(path).as_ref() == before.as_ref()
            })
    }
}

#[derive(PartialEq, Eq)]
enum Stamp {
    Missing,
    Directory(super::fingerprint::ObjectId),
    Present { len: u64, modified: SystemTime },
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        match std::fs::metadata(path) {
            // Directory churn (index writes, for example) does not change
            // config. Only a different directory identity invalidates it.
            Ok(metadata) if metadata.is_dir() => match super::Fingerprint::of(path) {
                super::Fingerprint::Directory(id) => Some(Self::Directory(id)),
                _ => None,
            },
            Ok(metadata) if metadata.is_file() => Some(Self::Present {
                len: metadata.len(),
                modified: metadata.modified().ok()?,
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => Some(Self::Missing),
            _ => None,
        }
    }
}
