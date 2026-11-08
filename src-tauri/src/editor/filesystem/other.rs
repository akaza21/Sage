use std::fs::File;

use crate::{
    error::{AppError, AppResult},
    workspace::{RootIdentity, Workspace},
};

pub(in crate::editor) struct HeldParent;

fn unavailable() -> AppError {
    AppError::Unsupported("safe editor saves are unavailable on this platform".into())
}

impl HeldParent {
    pub(in crate::editor) fn open(_workspace: &Workspace, _parent: &str) -> AppResult<Self> {
        Err(unavailable())
    }

    pub(in crate::editor) fn identity(&self) -> AppResult<RootIdentity> {
        Err(unavailable())
    }

    pub(in crate::editor) fn open_file(&self, _name: &str) -> AppResult<File> {
        Err(unavailable())
    }

    pub(super) fn create_new(&self, _name: &str) -> AppResult<File> {
        Err(unavailable())
    }

    pub(in crate::editor) fn replace(&self, _temp: &str, _target: &str) -> AppResult<()> {
        Err(unavailable())
    }

    pub(super) fn create_new_document(&self, _name: &str) -> AppResult<File> {
        Err(unavailable())
    }

    pub(in crate::editor) fn rename_new(&self, _temp: &str, _target: &str) -> AppResult<bool> {
        Err(unavailable())
    }

    pub(in crate::editor) fn remove(&self, _name: &str) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    pub(in crate::editor) fn sync(&self) -> AppResult<()> {
        Err(unavailable())
    }
}
