use std::sync::{Arc, Mutex};

use windows::{
    Win32::{
        Foundation::E_FAIL,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
        UI::Shell::{
            FILEOPERATION_FLAGS, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
            FOFX_ADDUNDORECORD, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation,
            IFileOperation, IFileOperationProgressSink, IFileOperationProgressSink_Impl,
            IShellItem, SHCreateItemFromIDList, SHParseDisplayName, SICHINT_CANONICAL,
        },
    },
    core::{HRESULT, PCWSTR, Ref, implement},
};

use crate::error::{AppError, AppResult};

use super::{ResourceTrashAuthority, verify_visible_source};

#[derive(Default)]
struct ProgressState {
    pre_delete_count: u8,
    post_delete_count: u8,
    post_delete_result: Option<HRESULT>,
    validation_error: Option<String>,
}

impl ProgressState {
    fn reject(&mut self, message: &str) -> windows::core::Result<()> {
        self.validation_error.get_or_insert_with(|| message.into());
        Err(E_FAIL.into())
    }

    fn record_pre(
        &mut self,
        item_matches: bool,
        visible_identity_valid: bool,
    ) -> windows::core::Result<()> {
        if !item_matches {
            return self
                .reject("Recycle Bin pre-delete callback item did not match the queued item");
        }
        if self.pre_delete_count != 0 || self.post_delete_count != 0 {
            return self.reject("Recycle Bin pre-delete callback was duplicated or out of order");
        }
        if !visible_identity_valid {
            return self.reject("Recycle Bin source identity changed before mutation");
        }
        self.pre_delete_count = 1;
        Ok(())
    }

    fn record_post(&mut self, item_matches: bool, result: HRESULT) -> windows::core::Result<()> {
        if !item_matches {
            return self
                .reject("Recycle Bin post-delete callback item did not match the queued item");
        }
        if self.pre_delete_count != 1 || self.post_delete_count != 0 {
            return self.reject("Recycle Bin post-delete callback was duplicated or out of order");
        }
        self.post_delete_count = 1;
        self.post_delete_result = Some(result);
        Ok(())
    }
}

#[implement(IFileOperationProgressSink)]
struct TrashProgressSink {
    authority: Arc<ResourceTrashAuthority>,
    queued_item: IShellItem,
    state: Arc<Mutex<ProgressState>>,
}

impl TrashProgressSink {
    fn item_matches(&self, item: &Ref<'_, IShellItem>) -> bool {
        same_shell_item_identity(&self.queued_item, item)
    }
}

fn same_shell_item_identity(queued_item: &IShellItem, item: &Ref<'_, IShellItem>) -> bool {
    item.as_ref()
        .and_then(|item| unsafe { queued_item.Compare(item, SICHINT_CANONICAL.0 as u32).ok() })
        .is_some_and(|comparison| comparison == 0)
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
        psiitem: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        let item_matches = self.item_matches(&psiitem);
        let visible_identity_valid = verify_visible_source(&self.authority).is_ok();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.record_pre(item_matches, visible_identity_valid)
    }

    fn PostDeleteItem(
        &self,
        _dwflags: u32,
        psiitem: Ref<'_, IShellItem>,
        hrdelete: HRESULT,
        _psinewlycreated: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        let item_matches = self.item_matches(&psiitem);
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_post(item_matches, hrdelete)
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
            | FOF_ALLOWUNDO.0
            | FOF_NOCONFIRMATION.0
            | FOF_NOERRORUI.0
            | FOF_SILENT.0,
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
    let canonical = std::fs::canonicalize(&authority.source_path).map_err(AppError::Io)?;
    let shell_path = canonical
        .to_string_lossy()
        .strip_prefix(r"\\?\")
        .map_or_else(|| canonical.to_string_lossy().into_owned(), str::to_owned);
    let path = shell_path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut pidl = std::ptr::null_mut();
    unsafe { SHParseDisplayName(PCWSTR(path.as_ptr()), None, &mut pidl, 0, None) }.map_err(
        |error| {
            AppError::InvalidRequest(format!("Recycle Bin Shell PIDL creation failed: {error}"))
        },
    )?;
    let item: IShellItem = unsafe { SHCreateItemFromIDList(pidl) }.map_err(|error| {
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
        queued_item: item.clone(),
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
    if state.pre_delete_count != 1 {
        return Err(AppError::InvalidRequest(
            "Recycle Bin pre-delete identity callback was not observed".into(),
        ));
    }
    let post = state.post_delete_result.ok_or_else(|| {
        AppError::InvalidRequest("Recycle Bin post-delete result was not observed".into())
    })?;
    if state.post_delete_count != 1 {
        return Err(AppError::InvalidRequest(
            "Recycle Bin post-delete callback count was not exact".into(),
        ));
    }
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

#[cfg(test)]
mod tests {
    use std::os::windows::ffi::OsStrExt;

    use super::*;
    use windows::Win32::UI::Shell::SHCreateItemFromParsingName;

    fn shell_item(path: &std::path::Path) -> windows::core::Result<IShellItem> {
        let wide_path = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide_path.as_ptr()), None) }
    }

    #[test]
    fn operation_flags_require_recycling_no_ui_and_undo() {
        let flags = operation_flags().0;
        for required in [
            FOFX_RECYCLEONDELETE.0,
            FOFX_EARLYFAILURE.0,
            FOFX_ADDUNDORECORD.0,
            FOF_ALLOWUNDO.0,
            FOF_NOCONFIRMATION.0,
            FOF_NOERRORUI.0,
            FOF_SILENT.0,
        ] {
            assert_eq!(flags & required, required);
        }
    }

    #[test]
    fn aborted_or_failed_item_is_never_accepted_as_complete() {
        let successful = ProgressState {
            pre_delete_count: 1,
            post_delete_count: 1,
            post_delete_result: Some(HRESULT(0)),
            validation_error: None,
        };
        assert!(validate_completion(false, &successful).is_ok());
        assert!(validate_completion(true, &successful).is_err());

        let failed_item = ProgressState {
            pre_delete_count: 1,
            post_delete_count: 1,
            post_delete_result: Some(E_FAIL),
            validation_error: None,
        };
        assert!(validate_completion(false, &failed_item).is_err());
    }

    #[test]
    fn callback_validator_rejects_null_mismatch_duplicate_and_out_of_order_items() {
        let mut null_pre = ProgressState::default();
        assert!(null_pre.record_pre(false, true).is_err());

        let mut changed = ProgressState::default();
        assert!(changed.record_pre(true, false).is_err());

        let mut duplicate = ProgressState::default();
        duplicate.record_pre(true, true).unwrap();
        assert!(duplicate.record_pre(true, true).is_err());

        let mut out_of_order = ProgressState::default();
        assert!(out_of_order.record_post(true, HRESULT(0)).is_err());

        let mut duplicate_post = ProgressState::default();
        duplicate_post.record_pre(true, true).unwrap();
        duplicate_post.record_post(true, HRESULT(0)).unwrap();
        assert!(duplicate_post.record_post(true, HRESULT(0)).is_err());

        let mut mismatched_post = ProgressState::default();
        mismatched_post.record_pre(true, true).unwrap();
        assert!(mismatched_post.record_post(false, HRESULT(0)).is_err());
    }

    #[test]
    fn shell_item_identity_uses_canonical_item_comparison() {
        let root =
            std::env::temp_dir().join(format!("sage-trash-com-identity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("first.rs"), "first\n").unwrap();
        std::fs::write(root.join("second.rs"), "second\n").unwrap();
        let sta_root = root.clone();

        let joined = std::thread::spawn(move || -> windows::core::Result<()> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
            }
            let _apartment = ComApartment;
            let first = shell_item(&sta_root.join("first.rs"))?;
            let second = shell_item(&sta_root.join("second.rs"))?;
            let same = Some(shell_item(&sta_root.join("first.rs"))?);
            let distinct = Some(second);
            let null: Option<IShellItem> = None;
            assert!(same_shell_item_identity(&first, &Ref::from(&same)));
            assert!(!same_shell_item_identity(&first, &Ref::from(&distinct)));
            assert!(!same_shell_item_identity(&first, &Ref::from(&null)));
            Ok(())
        })
        .join();

        let cleanup = std::fs::remove_dir_all(&root);
        joined.expect("Shell item identity STA panicked").unwrap();
        cleanup.unwrap();
    }
}
