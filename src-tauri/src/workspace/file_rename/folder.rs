use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
};

use super::{FileRenameCommit, Workspace};

impl Workspace {
    #[cfg(test)]
    pub(crate) fn rename_empty_folder(
        &self,
        source: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileRenameCommit> {
        self.rename_empty_folder_unavailable(source, basename)
    }

    pub(crate) fn rename_empty_folder_unavailable(
        &self,
        source: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileRenameCommit> {
        let _ = (self, source, basename);

        Err(AppError::Unsupported(
            "empty-folder rename is unavailable until Sage has an identity-scoped directory-mutation guard"
                .into(),
        ))
    }
}
