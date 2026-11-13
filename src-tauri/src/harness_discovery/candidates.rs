use std::{collections::HashSet, ffi::OsStr, path::PathBuf};

#[cfg(test)]
use super::HarnessExecutable;
use super::{
    DiscoverySource, InstallLayout, KNOWN_INSTALL_ROOT_CAP, KnownHarnessSpec,
    validation::{expected_basenames, is_local_probe_root},
};

#[derive(Clone)]
pub(super) struct Candidate {
    pub(super) executable: PathBuf,
    #[cfg(test)]
    pub(super) source: DiscoverySource,
}

fn candidate(executable: PathBuf, _source: DiscoverySource) -> Candidate {
    Candidate {
        executable,
        #[cfg(test)]
        source: _source,
    }
}

pub(super) fn production_present_candidates(
    specs: &[KnownHarnessSpec],
) -> Vec<(&'static str, Candidate)> {
    let mut names = Vec::new();
    for spec in specs {
        for basename in expected_basenames(spec.executable) {
            if !names.contains(basename) {
                names.push(*basename);
            }
        }
    }
    let path = scan_candidates_from_roots(
        &names,
        path_roots()
            .into_iter()
            .map(|root| (DiscoverySource::Path, root)),
    );
    let npm_global = scan_candidates_from_roots(
        &names,
        known_install_roots(InstallLayout::NpmGlobal)
            .into_iter()
            .map(|root| (DiscoverySource::KnownInstallRoot, root)),
    );
    let cursor_agent = scan_candidates_from_roots(
        &names,
        known_install_roots(InstallLayout::CursorAgent)
            .into_iter()
            .map(|root| (DiscoverySource::KnownInstallRoot, root)),
    );
    #[cfg(unix)]
    let native_claude = std::env::var_os("HOME")
        .as_deref()
        .and_then(|home| super::native_claude::launcher(std::path::Path::new(home)))
        .map(|path| candidate(path, DiscoverySource::KnownInstallRoot));
    specs
        .iter()
        .filter_map(|spec| {
            let fallback = match spec.install_layout {
                InstallLayout::NpmGlobal => &npm_global,
                InstallLayout::CursorAgent => &cursor_agent,
            };
            let found = path
                .iter()
                .chain(fallback)
                .find(|candidate| {
                    expected_basenames(spec.executable).iter().any(|name| {
                        candidate.executable.file_name() == Some(OsStr::new(name))
                            && super::validation::authentic_harness_file(
                                &candidate.executable,
                                name,
                            )
                    })
                })
                .cloned();
            #[cfg(unix)]
            let found = found.or_else(|| match spec.executable {
                super::HarnessExecutable::Claude => native_claude.clone(),
                _ => None,
            });
            found.map(|candidate| (spec.id, candidate))
        })
        .collect()
}

pub(super) fn path_candidates_for_basename(expected_basename: &str) -> Vec<PathBuf> {
    path_roots()
        .into_iter()
        .map(|root| root.join(expected_basename))
        .collect()
}

fn path_roots() -> Vec<PathBuf> {
    path_roots_from(std::env::var_os("PATH").as_deref())
}

pub(super) fn path_roots_from(path: Option<&OsStr>) -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = path
        .map(|path| std::env::split_paths(path).collect())
        .unwrap_or_default();
    bounded_path_roots(roots)
}

pub(super) fn bounded_path_roots(roots: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter_map(normalized_probe_root)
        .filter(|root| seen.insert(root.clone()))
        .collect()
}

fn normalized_probe_root(root: PathBuf) -> Option<PathBuf> {
    if !is_local_probe_root(&root) {
        return None;
    }
    let metadata = root.symlink_metadata().ok()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    let root = root.canonicalize().ok()?;
    is_local_probe_root(&root).then_some(root)
}

fn known_install_roots(install_layout: InstallLayout) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        known_install_roots_from_windows(
            install_layout,
            std::env::var_os("APPDATA").as_deref(),
            std::env::var_os("LOCALAPPDATA").as_deref(),
        )
    }
    #[cfg(not(windows))]
    {
        known_install_roots_from_home(install_layout, std::env::var_os("HOME").as_deref())
    }
}

#[cfg(windows)]
pub(crate) fn known_install_roots_from_windows(
    install_layout: InstallLayout,
    app_data: Option<&OsStr>,
    local_app_data: Option<&OsStr>,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = match install_layout {
        InstallLayout::CursorAgent => local_app_data
            .map(|root| PathBuf::from(root).join("cursor-agent"))
            .into_iter()
            .collect(),
        InstallLayout::NpmGlobal => app_data
            .map(|root| PathBuf::from(root).join("npm"))
            .into_iter()
            .collect(),
    };
    roots.retain(|root| is_local_probe_root(root));
    roots.into_iter().take(KNOWN_INSTALL_ROOT_CAP).collect()
}

#[cfg(windows)]
pub(crate) fn known_install_roots_from_app_data(app_data: Option<&OsStr>) -> Vec<PathBuf> {
    known_install_roots_from_windows(InstallLayout::NpmGlobal, app_data, None)
}

#[cfg(not(windows))]
fn known_install_roots_from_home(
    install_layout: InstallLayout,
    home: Option<&OsStr>,
) -> Vec<PathBuf> {
    if install_layout == InstallLayout::CursorAgent {
        return Vec::new();
    }
    let mut roots = home
        .into_iter()
        .flat_map(|home| {
            let home = PathBuf::from(home);
            [
                home.join(".npm-global").join("bin"),
                home.join(".local").join("bin"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
            ]
        })
        .filter(|root| is_local_probe_root(root))
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    roots.into_iter().take(KNOWN_INSTALL_ROOT_CAP).collect()
}

#[cfg(test)]
pub(super) fn candidates_from_roots(
    basenames: &[&str],
    roots: impl IntoIterator<Item = (DiscoverySource, PathBuf)>,
) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|(_, root)| seen.insert(root.clone()))
        .flat_map(|(source, root)| {
            basenames
                .iter()
                .map(move |basename| candidate(root.join(basename), source))
        })
        .collect()
}

pub(super) fn scan_candidates_from_roots(
    wanted_names: &[&str],
    roots: impl IntoIterator<Item = (DiscoverySource, PathBuf)>,
) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter_map(|(source, root)| {
            let root = normalized_probe_root(root)?;
            seen.insert(root.clone()).then_some((source, root))
        })
        .flat_map(|(source, root)| {
            wanted_names.iter().filter_map(move |name| {
                let executable = root.join(name);
                matches!(
                    executable.symlink_metadata(),
                    Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink()
                )
                .then(|| candidate(executable, source))
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
