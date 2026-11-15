use std::path::Path;

use super::{
    NativeResourceMapper, ResourceMapError, has_noncanonical_components, validate_path_bytes,
};
use sage_acp_protocol::intent::SafeResourceRef;

impl NativeResourceMapper {
    pub(crate) fn try_map_missing_display(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, ResourceMapError> {
        validate_path_bytes(absolute_path, false)?;
        if !Path::new(absolute_path).is_absolute() || absolute_path.contains("://") {
            return Err(ResourceMapError::PathNotAbsolute);
        }
        if has_noncanonical_components(absolute_path) || absolute_path.chars().any(char::is_control)
        {
            return Err(ResourceMapError::PathNotCanonical);
        }
        #[cfg(windows)]
        let supplied = {
            if absolute_path.starts_with("\\\\") || absolute_path.starts_with("//") {
                return Err(ResourceMapError::PathNotCanonical);
            }
            super::windows_comparison_spelling(absolute_path)
        };
        #[cfg(not(windows))]
        let supplied = absolute_path.to_owned();

        let mut selected = None;
        for root in &self.roots {
            #[cfg(windows)]
            let root_path = super::windows_comparison_spelling(
                root.native_root
                    .to_str()
                    .ok_or(ResourceMapError::RootNotCanonical)?,
            );
            #[cfg(not(windows))]
            let root_path = root
                .native_root
                .to_str()
                .ok_or(ResourceMapError::RootNotCanonical)?
                .to_owned();
            let root_path = root_path.trim_end_matches('/');

            #[cfg(windows)]
            let inside = supplied.len() > root_path.len()
                && supplied
                    .get(..root_path.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(root_path))
                && supplied.as_bytes()[root_path.len()] == b'/';
            #[cfg(not(windows))]
            let inside = supplied.starts_with(root_path)
                && supplied.as_bytes().get(root_path.len()) == Some(&b'/');
            if inside && selected.replace((root, root_path.len())).is_some() {
                return Err(ResourceMapError::AmbiguousRoot);
            }
        }
        let (root, prefix_len) = selected.ok_or(ResourceMapError::ForeignPath)?;
        let relative = supplied
            .get(prefix_len + 1..)
            .ok_or(ResourceMapError::InvalidResource)?;
        if relative.is_empty() || relative.len() > 512 {
            return Err(ResourceMapError::InvalidResource);
        }
        SafeResourceRef::new(root.root_id.clone(), relative)
            .map_err(|_| ResourceMapError::InvalidResource)
    }
}
