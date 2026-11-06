//! The OS confirmation Reveal and Copy ask for every time: Windows Hello
//! (face, fingerprint or PIN) over Sage's window. Sage's UI cannot answer it.

use super::{VaultError, VaultResult};

#[cfg(windows)]
pub(super) fn confirm_user(window: &tauri::Window, message: &str) -> VaultResult<()> {
    use windows::{
        Security::Credentials::UI::{
            UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
        },
        Win32::System::WinRT::IUserConsentVerifierInterop,
        core::{HSTRING, factory},
    };
    use windows_future::IAsyncOperation;

    let unavailable = |_| VaultError::ConfirmationUnavailable;
    let availability = UserConsentVerifier::CheckAvailabilityAsync()
        .and_then(|operation| operation.get())
        .map_err(unavailable)?;
    if availability != UserConsentVerifierAvailability::Available {
        return Err(VaultError::ConfirmationUnavailable);
    }
    let owner = window
        .hwnd()
        .map_err(|_| VaultError::ConfirmationUnavailable)?;
    let interop =
        factory::<UserConsentVerifier, IUserConsentVerifierInterop>().map_err(unavailable)?;
    // SAFETY: `owner` is Sage's live main window for the whole prompt.
    let operation: IAsyncOperation<UserConsentVerificationResult> =
        unsafe { interop.RequestVerificationForWindowAsync(owner, &HSTRING::from(message)) }
            .map_err(unavailable)?;
    match operation.get() {
        Ok(UserConsentVerificationResult::Verified) => Ok(()),
        _ => Err(VaultError::NotConfirmed),
    }
}

#[cfg(not(windows))]
pub(super) fn confirm_user(_window: &tauri::Window, _message: &str) -> VaultResult<()> {
    Err(VaultError::Unsupported)
}
