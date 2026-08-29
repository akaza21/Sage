use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use super::{
    super::{
        authority::{CodexAuthorityPrerequisite, CodexLaunchEnvironment},
        installed_role::SignedInstalledRole,
        manifest::MetadataValidatedPacket,
        reproduction::ReproducedPackagePrerequisite,
    },
    AdmittedCodexBinding, CodexAdmissionUnavailable,
};

#[derive(Debug)]
pub(crate) enum CodexRegistrationState {
    Admitted(Arc<AdmittedCodexBinding>),
    Unavailable(CodexAdmissionUnavailable),
}

#[derive(Debug)]
pub(crate) struct CodexRegistrationAuthority {
    pub(super) state: Mutex<CodexRegistrationState>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexBindingRegistration {
    pub(super) authority: Arc<CodexRegistrationAuthority>,
}

impl CodexBindingRegistration {
    pub(crate) fn authenticate_observed(
        &self,
        executable: PathBuf,
        version: String,
    ) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
        let candidate = Arc::new(AdmittedCodexBinding::admit_observed(executable, version)?);
        let mut state = self.lock_state();
        match &*state {
            CodexRegistrationState::Unavailable(
                CodexAdmissionUnavailable::IndependentAdmission,
            )
            | CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::Platform) => {
                *state = CodexRegistrationState::Admitted(Arc::clone(&candidate));
                Ok(candidate)
            }
            CodexRegistrationState::Unavailable(reason) => Err(*reason),
            CodexRegistrationState::Admitted(binding) => Ok(Arc::clone(binding)),
        }
    }

    pub(crate) fn authenticate_signed_installed_role(
        &self,
        packet: &MetadataValidatedPacket,
        role: SignedInstalledRole,
        reproduced: ReproducedPackagePrerequisite,
        authority: CodexAuthorityPrerequisite,
        environment: CodexLaunchEnvironment,
    ) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
        let candidate = Arc::new(
            AdmittedCodexBinding::admit_signed_installed_role(
                packet.clone(),
                role,
                reproduced,
                authority,
                environment,
            )
            .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?,
        );
        let mut state = self.lock_state();
        match &*state {
            CodexRegistrationState::Unavailable(
                CodexAdmissionUnavailable::IndependentAdmission,
            ) => {
                *state = CodexRegistrationState::Admitted(Arc::clone(&candidate));
                Ok(candidate)
            }
            CodexRegistrationState::Unavailable(reason) => Err(*reason),
            CodexRegistrationState::Admitted(binding) => Ok(Arc::clone(binding)),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(*self.lock_state(), CodexRegistrationState::Admitted(_))
    }

    pub(crate) fn unavailable(&self) -> Option<CodexAdmissionUnavailable> {
        match *self.lock_state() {
            CodexRegistrationState::Admitted(_) => None,
            CodexRegistrationState::Unavailable(reason) => Some(reason),
        }
    }

    pub(crate) fn refresh(&self) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
        let candidate = match &*self.lock_state() {
            CodexRegistrationState::Admitted(binding) => Arc::clone(binding),
            CodexRegistrationState::Unavailable(reason) => return Err(*reason),
        };
        let validation = candidate.revalidate();
        let mut state = self.lock_state();
        match &*state {
            CodexRegistrationState::Admitted(current) if Arc::ptr_eq(current, &candidate) => {
                if validation.is_err() {
                    *state = CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::Drift);
                    Err(CodexAdmissionUnavailable::Drift)
                } else {
                    Ok(candidate)
                }
            }
            CodexRegistrationState::Admitted(_) => Err(CodexAdmissionUnavailable::Drift),
            CodexRegistrationState::Unavailable(reason) => Err(*reason),
        }
    }

    pub(crate) fn invalidate_drift(&self) {
        *self.lock_state() = CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::Drift);
    }

    pub(super) fn lock_state(&self) -> MutexGuard<'_, CodexRegistrationState> {
        match self.authority.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                *state = CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::Drift);
                state
            }
        }
    }

    /// Authenticode can identify the installed role, but this workstation's
    /// curated evidence records it as observation-only. If independent launch
    /// admission cannot proceed, retain that more precise fail-closed state.
    pub(super) fn refuse_independent_launch_after_workstation_authenticode_observation(
        &self,
    ) -> CodexAdmissionUnavailable {
        let mut state = self.lock_state();
        let unavailable = match &*state {
            CodexRegistrationState::Unavailable(reason) => *reason,
            CodexRegistrationState::Admitted(_) => return CodexAdmissionUnavailable::Drift,
        };
        if unavailable == CodexAdmissionUnavailable::IndependentAdmission {
            *state =
                CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::ObservationOnly);
            CodexAdmissionUnavailable::ObservationOnly
        } else {
            unavailable
        }
    }
}
