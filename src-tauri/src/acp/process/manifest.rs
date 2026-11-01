use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::supervisor::{LaunchError, validate_absolute_normalized};

pub(crate) const MANIFEST_BYTES_MAX: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestExpectation {
    canonical_absolute_path: PathBuf,
    sha256: [u8; 32],
    byte_len: u64,
}

impl ManifestExpectation {
    #[cfg_attr(
        not(windows),
        allow(
            dead_code,
            reason = "only the Windows supervisor pins launch manifests"
        )
    )]
    pub(super) fn open_for_launch(
        spec: &super::supervisor::LaunchSpec,
    ) -> Result<Option<ManifestGuard>, LaunchError> {
        spec.artifact
            .manifest
            .as_ref()
            .map(Self::open_guard)
            .transpose()
    }

    pub(crate) fn new(
        canonical_absolute_path: PathBuf,
        sha256: [u8; 32],
        byte_len: u64,
    ) -> Result<Self, LaunchError> {
        validate_absolute_normalized(&canonical_absolute_path)?;
        if byte_len > MANIFEST_BYTES_MAX {
            return Err(LaunchError::IdentityMismatch);
        }
        Ok(Self {
            canonical_absolute_path,
            sha256,
            byte_len,
        })
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.canonical_absolute_path
    }

    #[cfg_attr(
        not(windows),
        allow(
            dead_code,
            reason = "only the Windows supervisor pins launch manifests"
        )
    )]
    pub(crate) fn open_guard(&self) -> Result<ManifestGuard, LaunchError> {
        let mut file = open_denying_writers(&self.canonical_absolute_path)?;
        let mut bytes = Vec::with_capacity(usize::try_from(self.byte_len).unwrap_or(0));
        file.by_ref()
            .take(self.byte_len.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| LaunchError::Io)?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != self.byte_len || digest != self.sha256
        {
            tracing::warn!(
                path = ?self.canonical_absolute_path,
                expected_bytes = self.byte_len,
                observed_bytes = bytes.len(),
                "ACP launch package manifest changed after admission"
            );
            return Err(LaunchError::IdentityMismatch);
        }
        Ok(ManifestGuard { file })
    }
}

#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "only the Windows supervisor pins launch manifests"
    )
)]
#[must_use = "dropping the manifest guard reopens the substitution window"]
pub(crate) struct ManifestGuard {
    #[allow(dead_code, reason = "held open to deny writers until the guard drops")]
    file: File,
}

impl ManifestGuard {
    #[cfg(test)]
    pub(crate) fn byte_len(&self) -> Result<u64, LaunchError> {
        Ok(self.file.metadata().map_err(|_| LaunchError::Io)?.len())
    }
}

#[cfg(windows)]
fn open_denying_writers(path: &Path) -> Result<File, LaunchError> {
    super::windows::artifact::open_pinned_regular_file(path)
}

#[cfg(not(windows))]
#[allow(
    dead_code,
    reason = "only the Windows supervisor pins launch manifests"
)]
fn open_denying_writers(path: &Path) -> Result<File, LaunchError> {
    File::open(path).map_err(|_| LaunchError::Io)
}

#[cfg(test)]
mod tests {
    use super::{MANIFEST_BYTES_MAX, ManifestExpectation};
    use crate::acp::process::supervisor::LaunchError;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    struct TempFile(PathBuf);

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn recorded(label: &str, bytes: &[u8]) -> (TempFile, ManifestExpectation) {
        let path = TempFile(
            crate::test_paths::temp_dir()
                .join(format!("sage-manifest-{label}-{}", uuid::Uuid::new_v4())),
        );
        std::fs::write(&path.0, bytes).expect("write manifest");
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        let expectation = ManifestExpectation::new(
            path.0.clone(),
            digest,
            u64::try_from(bytes.len()).expect("length"),
        )
        .expect("expectation");
        (path, expectation)
    }

    #[test]
    fn guard_accepts_the_recorded_manifest_and_refuses_every_later_edit() {
        let (path, expectation) = recorded("edit", br#"{"name":"opencode-ai"}"#);
        let guard = expectation.open_guard().expect("guard");
        assert_eq!(guard.byte_len(), Ok(22));
        drop(guard);

        std::fs::write(&path.0, br#"{"name":"evil-ai"}"#).expect("rewrite manifest");

        assert_eq!(
            expectation.open_guard().err(),
            Some(LaunchError::IdentityMismatch)
        );
    }

    #[test]
    fn guard_refuses_a_truncated_manifest_and_a_missing_manifest() {
        let (path, expectation) = recorded("truncated", br#"{"name":"opencode-ai"}"#);
        std::fs::write(&path.0, br#"{"name":"opencode-ai""#).expect("truncate manifest");
        assert_eq!(
            expectation.open_guard().err(),
            Some(LaunchError::IdentityMismatch)
        );

        std::fs::remove_file(&path.0).expect("remove manifest");
        assert_eq!(expectation.open_guard().err(), Some(LaunchError::Io));
    }

    #[test]
    fn expectation_refuses_relative_paths_and_manifests_beyond_the_admission_bound() {
        assert_eq!(
            ManifestExpectation::new(PathBuf::from("package.json"), [0_u8; 32], 4).err(),
            Some(LaunchError::InvalidAbsolutePath)
        );
        let (_path, expectation) = recorded("bound", br#"{"name":"opencode-ai"}"#);
        assert_eq!(
            ManifestExpectation::new(
                expectation.path().to_path_buf(),
                [0_u8; 32],
                MANIFEST_BYTES_MAX + 1,
            )
            .err(),
            Some(LaunchError::IdentityMismatch)
        );
    }
}
