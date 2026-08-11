use std::sync::{Arc, Mutex};

use windows::{
    Win32::{
        Foundation::E_FAIL,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
        UI::Shell::{
            FILEOPERATION_FLAGS, FOF_NO_UI, FOF_NOCONFIRMATION, FOF_NORECURSION,
            FOFX_ADDUNDORECORD, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation,
            IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl,
            IShellItem, SHCreateItemFromParsingName,
        },
    },
    core::{HRESULT, PCWSTR, Ref, implement},
};

use crate::error::{AppError, AppResult};

use super::{ResourceTrashAuthority, verify_visible_source};

#[derive(Default)]
struct ProgressState {
    pre_delete_seen: bool,
    post_delete_result: Option<HRESULT>,
    validation_error: Option<String>,
}

#[implement(IFileOperationProgressSink)]
struct TrashProgressSink {
    authority: Arc<ResourceTrashAuthority>,
    state: Arc<Mutex<ProgressState>>,
}

#[allow(non_snake_case)]
impl IFileOperationProgressSink_Impl for TrashProgressSink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn FinishOperations(&self, _hrresult: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }

    fn PreDeleteItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match verify_visible_source(&self.authority) {
            Ok(()) => {
                state.pre_delete_seen = true;
                Ok(())
            }
            Err(error) => {
                state.validation_error = Some(error.to_string());
                Err(E_FAIL.into())
            }
        }
    }

    fn PostDeleteItem(
        &self,
        _dwflags: u32,
        _psiitem: Ref<'_, IShellItem>,
        hrdelete: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .post_delete_result = Some(hrdelete);
        Ok(())
    }

    fn PreRenameItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

fn operation_flags() -> FILEOPERATION_FLAGS {
    FILEOPERATION_FLAGS(
        FOFX_RECYCLEONDELETE.0
            | FOFX_EARLYFAILURE.0
            | FOFX_ADDUNDORECORD.0
            | FOF_NO_UI.0
            | FOF_NOCONFIRMATION.0
            | FOF_NORECURSION.0,
    )
}

struct ComApartment;

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: this guard is constructed only after this thread initialized COM.
        unsafe { CoUninitialize() };
    }
}

fn run_shell_operation<C>(
    authority: Arc<ResourceTrashAuthority>,
    mutation_started: C,
) -> AppResult<()>
where
    C: FnOnce(Arc<ResourceTrashAuthority>),
{
    // SAFETY: the dedicated thread owns this STA for the lifetime of all COM values below.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
            .ok()
            .map_err(|error| {
                AppError::InvalidRequest(format!("Recycle Bin COM initialization failed: {error}"))
            })?;
    }
    let _apartment = ComApartment;
    let path = authority
        .source_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }
        .map_err(|error| {
            AppError::InvalidRequest(format!("Recycle Bin Shell item creation failed: {error}"))
        })?;
    let operation: IFileOperation =
        unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER) }.map_err(
            |error| {
                AppError::InvalidRequest(format!("Recycle Bin operation creation failed: {error}"))
            },
        )?;
    unsafe { operation.SetOperationFlags(operation_flags()) }.map_err(|error| {
        AppError::InvalidRequest(format!("Recycle Bin flags were rejected: {error}"))
    })?;
    let state = Arc::new(Mutex::new(ProgressState::default()));
    let sink: IFileOperationProgressSink = TrashProgressSink {
        authority: Arc::clone(&authority),
        state: Arc::clone(&state),
    }
    .into();
    unsafe { operation.DeleteItem(&item, &sink) }.map_err(|error| {
        AppError::InvalidRequest(format!("Recycle Bin item queue failed: {error}"))
    })?;
    mutation_started(authority);
    let performed = unsafe { operation.PerformOperations() };
    let aborted = unsafe { operation.GetAnyOperationsAborted() }.map_err(|error| {
        AppError::InvalidRequest(format!("Recycle Bin abort status failed: {error}"))
    })?;
    performed.map_err(|error| {
        AppError::InvalidRequest(format!("Recycle Bin operation failed: {error}"))
    })?;
    let state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    validate_completion(aborted.as_bool(), &state)
}

fn validate_completion(aborted: bool, state: &ProgressState) -> AppResult<()> {
    if let Some(error) = &state.validation_error {
        return Err(AppError::InvalidRequest(error.clone()));
    }
    if aborted {
        return Err(AppError::InvalidRequest(
            "Recycle Bin operation was aborted before completion".into(),
        ));
    }
    if !state.pre_delete_seen {
        return Err(AppError::InvalidRequest(
            "Recycle Bin pre-delete identity callback was not observed".into(),
        ));
    }
    let post = state.post_delete_result.ok_or_else(|| {
        AppError::InvalidRequest("Recycle Bin post-delete result was not observed".into())
    })?;
    post.ok().map_err(|error| {
        AppError::InvalidRequest(format!("Recycle Bin item deletion failed: {error}"))
    })
}

pub(super) fn recycle_on_sta<C>(
    authority: Arc<ResourceTrashAuthority>,
    mutation_started: C,
) -> AppResult<()>
where
    C: FnOnce(Arc<ResourceTrashAuthority>) + Send + 'static,
{
    std::thread::Builder::new()
        .name("sage-recycle-bin".into())
        .spawn(move || run_shell_operation(authority, mutation_started))
        .map_err(AppError::Io)?
        .join()
        .map_err(|_| AppError::InvalidRequest("Recycle Bin STA thread panicked".into()))?
}

use std::os::windows::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_flags_require_recycling_no_ui_no_recursion_and_undo() {
        let flags = operation_flags().0;
        for required in [
            FOFX_RECYCLEONDELETE.0,
            FOFX_EARLYFAILURE.0,
            FOFX_ADDUNDORECORD.0,
            FOF_NO_UI.0,
            FOF_NOCONFIRMATION.0,
            FOF_NORECURSION.0,
        ] {
            assert_eq!(flags & required, required);
        }
    }

    #[test]
    fn aborted_or_failed_item_is_never_accepted_as_complete() {
        let successful = ProgressState {
            pre_delete_seen: true,
            post_delete_result: Some(HRESULT(0)),
            validation_error: None,
        };
        assert!(validate_completion(false, &successful).is_ok());
        assert!(validate_completion(true, &successful).is_err());

        let failed_item = ProgressState {
            pre_delete_seen: true,
            post_delete_result: Some(E_FAIL),
            validation_error: None,
        };
        assert!(validate_completion(false, &failed_item).is_err());
    }
}
