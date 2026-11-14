use std::path::{Component, Path, PathBuf};

use crate::{
    project_registry::MAX_ROOTS_PER_PROJECT,
    workspace::{RootIdentity, leaf_identity, open_workspace_text_file},
};

use super::{
    bindings::adapter::ResourceMapper,
    contract::SageRootId,
    intent::{SafeResourceRef, UnavailableResourceReason},
};
use sage_core::limits::LIMITS;

const MAX_PROJECT_ROOTS: usize = MAX_ROOTS_PER_PROJECT as usize;
const MAX_ROOT_PATH_BYTES_TOTAL: usize =
    MAX_PROJECT_ROOTS * LIMITS.wire.absolute_path_bytes_native_only as usize;

mod diff_context;
mod missing_display;

/// Native-only configuration input. Absolute paths never cross the IPC or
/// durable normalized-item boundary.
pub(crate) struct ConfiguredResourceRoot {
    pub(crate) root_id: SageRootId,
    pub(crate) canonical_native_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceMapError {
    TooManyRoots,
    RootPathBytes,
    RootNotAbsolute,
    RootMissing,
    RootNotDirectory,
    RootNotCanonical,
    PathBytes,
    PathNotAbsolute,
    PathNotCanonical,
    PathMissing,
    ForeignPath,
    AmbiguousRoot,
    IdentityChanged,
    InvalidResource,
}

#[derive(Clone)]
struct CanonicalRoot {
    root_id: SageRootId,
    native_root: PathBuf,
    identity: RootIdentity,
}

/// Native-only proof that binds P-02's logical resource to the root and leaf
/// identities observed at absolute-path ingress. It must not cross IPC or a
/// durable receipt boundary.
#[derive(Clone)]
pub(crate) struct NativeResourceAdmission {
    resource: SafeResourceRef,
    root_identity: RootIdentity,
    file_identity: RootIdentity,
    roots: Vec<AdmittedRootIdentity>,
}

#[derive(Clone, PartialEq, Eq)]
struct AdmittedRootIdentity {
    root_id: SageRootId,
    canonical_native_root: PathBuf,
    identity: RootIdentity,
}

impl NativeResourceAdmission {
    pub(crate) fn resource(&self) -> &SafeResourceRef {
        &self.resource
    }

    pub(crate) fn root_identity(&self) -> RootIdentity {
        self.root_identity
    }

    pub(crate) fn file_identity(&self) -> RootIdentity {
        self.file_identity
    }
}

/// Immutable per-project root snapshot used by harness adapters. Constructing
/// it is the only filesystem work performed for configured roots; mapping an
/// event never reads SQLite or performs ambient project lookup.
pub(crate) struct NativeResourceMapper {
    roots: Vec<CanonicalRoot>,
}

impl NativeResourceMapper {
    pub(crate) fn new(
        roots: impl IntoIterator<Item = ConfiguredResourceRoot>,
    ) -> Result<Self, ResourceMapError> {
        let mut canonical_roots = Vec::new();
        canonical_roots
            .try_reserve_exact(MAX_PROJECT_ROOTS)
            .map_err(|_| ResourceMapError::RootPathBytes)?;
        let mut retained_path_bytes = 0usize;

        for configured in roots {
            if canonical_roots.len() == MAX_PROJECT_ROOTS {
                return Err(ResourceMapError::TooManyRoots);
            }
            let configured_text = configured
                .canonical_native_root
                .to_str()
                .ok_or(ResourceMapError::RootNotCanonical)?;
            validate_path_bytes(configured_text, true)?;
            retained_path_bytes = retained_path_bytes
                .checked_add(configured_text.len())
                .filter(|bytes| *bytes <= MAX_ROOT_PATH_BYTES_TOTAL)
                .ok_or(ResourceMapError::RootPathBytes)?;
            if !configured.canonical_native_root.is_absolute() {
                return Err(ResourceMapError::RootNotAbsolute);
            }
            let actual = configured
                .canonical_native_root
                .canonicalize()
                .map_err(|_| ResourceMapError::RootMissing)?;
            if !actual.is_dir() {
                return Err(ResourceMapError::RootNotDirectory);
            }
            if !canonical_spelling_matches(configured_text, &actual) {
                return Err(ResourceMapError::RootNotCanonical);
            }
            canonical_roots.push(CanonicalRoot {
                root_id: configured.root_id,
                identity: crate::workspace::root_identity_path(&actual)
                    .map_err(|_| ResourceMapError::RootMissing)?,
                native_root: actual,
            });
        }
        // Root registration order is not authority. A stable ordering makes a
        // later fresh mapper comparison reject only a real root-set change.
        canonical_roots.sort_by(|left, right| left.root_id.as_str().cmp(right.root_id.as_str()));

        Ok(Self {
            roots: canonical_roots,
        })
    }

    pub(crate) fn try_map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, ResourceMapError> {
        Ok(self.try_admit_existing(absolute_path)?.resource)
    }

    /// Maps an existing P-02 path and captures the root/leaf handles' native
    /// identities. The final handle is the same no-follow component walk used
    /// for bounded reads, never a canonicalize-then-reopen approximation.
    pub(crate) fn try_admit_existing(
        &self,
        absolute_path: &str,
    ) -> Result<NativeResourceAdmission, ResourceMapError> {
        validate_path_bytes(absolute_path, false)?;
        let supplied = Path::new(absolute_path);
        if !supplied.is_absolute() {
            return Err(ResourceMapError::PathNotAbsolute);
        }
        if has_noncanonical_components(absolute_path) {
            return Err(ResourceMapError::PathNotCanonical);
        }

        let actual = supplied
            .canonicalize()
            .map_err(|_| ResourceMapError::PathMissing)?;
        if !canonical_spelling_matches(absolute_path, &actual) {
            return Err(ResourceMapError::PathNotCanonical);
        }

        let root = self.root_for(&actual)?;
        let relative = actual
            .strip_prefix(&root.native_root)
            .map_err(|_| ResourceMapError::ForeignPath)?;
        let relative = canonical_relative_path(relative)?;
        let resource = SafeResourceRef::new(root.root_id.clone(), relative)
            .map_err(|_| ResourceMapError::InvalidResource)?;
        let file = open_workspace_text_file(
            &root.native_root,
            root.identity,
            resource.relative_path.as_str(),
            None,
        )
        .map_err(|_| ResourceMapError::IdentityChanged)?;
        let metadata = file
            .metadata()
            .map_err(|_| ResourceMapError::IdentityChanged)?;
        if !metadata.is_file() {
            return Err(ResourceMapError::PathMissing);
        }
        let file_identity =
            leaf_identity(&file, &metadata).map_err(|_| ResourceMapError::IdentityChanged)?;
        Ok(NativeResourceAdmission {
            resource,
            root_identity: root.identity,
            file_identity,
            roots: self
                .roots
                .iter()
                .map(|root| AdmittedRootIdentity {
                    root_id: root.root_id.clone(),
                    canonical_native_root: root.native_root.clone(),
                    identity: root.identity,
                })
                .collect(),
        })
    }

    /// Verifies an ingress admission using a freshly constructed mapper. This
    /// intentionally reconstructs only the admitted root plus relative path;
    /// callers cannot redirect revalidation through a new absolute spelling.
    pub(crate) fn revalidate_admission(
        &self,
        admission: &NativeResourceAdmission,
    ) -> Result<(), ResourceMapError> {
        let roots: Vec<_> = self
            .roots
            .iter()
            .map(|root| AdmittedRootIdentity {
                root_id: root.root_id.clone(),
                canonical_native_root: root.native_root.clone(),
                identity: root.identity,
            })
            .collect();
        if roots != admission.roots {
            return Err(ResourceMapError::IdentityChanged);
        }
        let root = self
            .roots
            .iter()
            .find(|root| root.root_id == admission.resource.root_id)
            .ok_or(ResourceMapError::IdentityChanged)?;
        let path = root
            .native_root
            .join(admission.resource.relative_path.as_str());
        let path = path.to_str().ok_or(ResourceMapError::PathNotCanonical)?;
        let current = self.try_admit_existing(path)?;
        if current.resource != admission.resource
            || current.root_identity != admission.root_identity
            || current.file_identity != admission.file_identity
        {
            return Err(ResourceMapError::IdentityChanged);
        }
        Ok(())
    }

    fn root_for(&self, actual: &Path) -> Result<&CanonicalRoot, ResourceMapError> {
        let mut selected = None;
        for (index, root) in self.roots.iter().enumerate() {
            if actual.strip_prefix(&root.native_root).is_ok() && selected.replace(index).is_some() {
                return Err(ResourceMapError::AmbiguousRoot);
            }
        }
        selected
            .map(|index| &self.roots[index])
            .ok_or(ResourceMapError::ForeignPath)
    }
}

impl ResourceMapper for NativeResourceMapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        self.try_map_existing(absolute_path)
            .map_err(ResourceMapError::unavailable_reason)
    }

    fn map_missing_display(&self, absolute_path: &str) -> Option<SafeResourceRef> {
        self.try_map_missing_display(absolute_path).ok()
    }

    fn read_diff_context(&self, resource: &SafeResourceRef) -> Option<String> {
        self.bounded_diff_context(resource)
    }
}

impl ResourceMapError {
    fn unavailable_reason(self) -> UnavailableResourceReason {
        match self {
            Self::AmbiguousRoot => UnavailableResourceReason::AmbiguousRoot,
            Self::ForeignPath => UnavailableResourceReason::Foreign,
            Self::PathMissing | Self::RootMissing => UnavailableResourceReason::Missing,
            Self::PathNotCanonical
            | Self::PathNotAbsolute
            | Self::RootNotCanonical
            | Self::RootNotAbsolute
            | Self::IdentityChanged => UnavailableResourceReason::Noncanonical,
            Self::TooManyRoots
            | Self::RootPathBytes
            | Self::RootNotDirectory
            | Self::PathBytes
            | Self::InvalidResource => UnavailableResourceReason::Unsupported,
        }
    }
}

fn validate_path_bytes(path: &str, root: bool) -> Result<(), ResourceMapError> {
    if path.len() <= LIMITS.wire.absolute_path_bytes_native_only as usize {
        Ok(())
    } else if root {
        Err(ResourceMapError::RootPathBytes)
    } else {
        Err(ResourceMapError::PathBytes)
    }
}

#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) fn validate_native_path_bytes(path: &str) -> Result<(), ResourceMapError> {
    validate_path_bytes(path, false)
}

fn canonical_relative_path(relative: &Path) -> Result<String, ResourceMapError> {
    let limit = LIMITS.wire.relative_path_bytes_native_only as usize;
    let mut result = String::new();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err(ResourceMapError::InvalidResource);
        };
        let value = value.to_str().ok_or(ResourceMapError::InvalidResource)?;
        let separator = usize::from(!result.is_empty());
        let next_len = result
            .len()
            .checked_add(separator)
            .and_then(|bytes| bytes.checked_add(value.len()))
            .filter(|bytes| *bytes <= limit)
            .ok_or(ResourceMapError::InvalidResource)?;
        result
            .try_reserve_exact(next_len - result.len())
            .map_err(|_| ResourceMapError::InvalidResource)?;
        if separator == 1 {
            result.push('/');
        }
        result.push_str(value);
    }
    if result.is_empty() {
        return Err(ResourceMapError::InvalidResource);
    }
    Ok(result)
}

#[cfg(not(windows))]
fn canonical_spelling_matches(supplied: &str, actual: &Path) -> bool {
    actual.to_str() == Some(supplied)
}

#[cfg(windows)]
fn canonical_spelling_matches(supplied: &str, actual: &Path) -> bool {
    let Some(actual) = actual.to_str() else {
        return false;
    };
    windows_comparison_spelling(supplied).eq_ignore_ascii_case(&windows_comparison_spelling(actual))
}

#[cfg(windows)]
fn windows_comparison_spelling(path: &str) -> String {
    let path = path.replace('\\', "/");
    if let Some(rest) = path.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = path.strip_prefix("//?/") {
        rest.to_owned()
    } else {
        path
    }
}

fn has_noncanonical_components(path: &str) -> bool {
    #[cfg(windows)]
    let path = windows_comparison_spelling(path);
    #[cfg(not(windows))]
    let path = path.to_owned();

    let body = if cfg!(windows) && path.starts_with("//") {
        &path[2..]
    } else {
        path.as_str()
    };
    body.ends_with('/')
        || body.contains("//")
        || body.split('/').any(|segment| matches!(segment, "." | ".."))
}

#[cfg(test)]
mod tests;
