use std::{collections::HashSet, ffi::OsStr, path::PathBuf};

use super::{
    DiscoverySource, HarnessExecutable, KNOWN_INSTALL_ROOT_CAP, PATH_DIR_CAP,
    validation::{expected_basenames, is_local_probe_root},
};

#[derive(Clone)]
pub(super) struct Candidate {
    pub(super) executable: PathBuf,
    pub(super) source: DiscoverySource,
}

pub(super) fn production_candidates(harness: HarnessExecutable) -> Vec<Candidate> {
    let path_roots = path_roots()
        .into_iter()
        .map(|root| (DiscoverySource::Path, root));
    let known_roots = known_install_roots()
        .into_iter()
        .map(|root| (DiscoverySource::KnownInstallRoot, root));
    candidates_from_roots(expected_basenames(harness), path_roots.chain(known_roots))
}

pub(super) fn path_candidates_for_basename(expected_basename: &str) -> Vec<PathBuf> {
    path_roots()
        .into_iter()
        .map(|root| root.join(expected_basename))
        .collect()
}

pub(super) fn legacy_candidates(command: &str) -> Vec<Candidate> {
    #[cfg(windows)]
    let basenames = [format!("{command}.exe"), format!("{command}.cmd")];
    #[cfg(not(windows))]
    let basenames = [command.to_owned()];
    let mut seen = HashSet::new();
    path_roots()
        .into_iter()
        .map(|root| (DiscoverySource::Path, root))
        .chain(
            known_install_roots()
                .into_iter()
                .map(|root| (DiscoverySource::KnownInstallRoot, root)),
        )
        .filter(|(_, root)| seen.insert(root.clone()))
        .flat_map(|(source, root)| {
            basenames.iter().map(move |basename| Candidate {
                executable: root.join(basename),
                source,
            })
        })
        .collect()
}

fn path_roots() -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    bounded_path_roots(roots)
}

pub(super) fn bounded_path_roots(roots: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    roots
        .into_iter()
        .filter(|root| is_local_probe_root(root))
        .take(PATH_DIR_CAP)
        .collect()
}

fn known_install_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        known_install_roots_from_app_data(std::env::var_os("APPDATA").as_deref())
    }
    #[cfg(not(windows))]
    {
        known_install_roots_from_home(std::env::var_os("HOME").as_deref())
    }
}

#[cfg(windows)]
pub(crate) fn known_install_roots_from_app_data(app_data: Option<&OsStr>) -> Vec<PathBuf> {
    app_data
        .map(|app_data| PathBuf::from(app_data).join("npm"))
        .filter(|root| is_local_probe_root(root))
        .into_iter()
        .take(KNOWN_INSTALL_ROOT_CAP)
        .collect()
}

#[cfg(not(windows))]
fn known_install_roots_from_home(home: Option<&OsStr>) -> Vec<PathBuf> {
    home.map(|home| PathBuf::from(home).join(".npm-global").join("bin"))
        .filter(|root| is_local_probe_root(root))
        .into_iter()
        .take(KNOWN_INSTALL_ROOT_CAP)
        .collect()
}

pub(super) fn candidates_from_roots(
    basenames: &[&str],
    roots: impl IntoIterator<Item = (DiscoverySource, PathBuf)>,
) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|(_, root)| seen.insert(root.clone()))
        .flat_map(|(source, root)| {
            basenames.iter().map(move |basename| Candidate {
                executable: root.join(basename),
                source,
            })
        })
        .collect()
}

#[cfg(test)]
pub(super) fn candidates_for_test(
    harness: HarnessExecutable,
    roots: impl IntoIterator<Item = (DiscoverySource, PathBuf)>,
) -> Vec<Candidate> {
    candidates_from_roots(expected_basenames(harness), roots)
}
