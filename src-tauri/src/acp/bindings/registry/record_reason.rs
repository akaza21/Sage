//! Why a registered binding is not admitted, projected into the catalog's
//! redacted reason.

use crate::domain::HarnessUnavailableReason;

use super::{Admission, BindingRecord};

impl BindingRecord {
    pub(crate) fn unavailable_reason(&self) -> Option<HarnessUnavailableReason> {
        use crate::acp::bindings::claude::ClaudeAdmissionUnavailable as Claude;
        use crate::acp::bindings::codex::CodexAdmissionUnavailable as Codex;
        use crate::acp::bindings::standard_acp::OpenCodeAdmissionUnavailable as OpenCode;
        match &self.admission {
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            Admission::Pi(binding) => binding
                .revalidate()
                .err()
                .map(|_| HarnessUnavailableReason::DriftDetected),
            Admission::Codex(admission) => admission.unavailable().map(|reason| match reason {
                Codex::Platform => HarnessUnavailableReason::PlatformUnsupported,
                Codex::ObservationOnly => HarnessUnavailableReason::ObservationOnly,
                Codex::IndependentAdmission => HarnessUnavailableReason::AdmissionIncomplete,
                Codex::Drift => HarnessUnavailableReason::DriftDetected,
                Codex::Projection(_) => HarnessUnavailableReason::CapabilityProjectionUnavailable,
            }),
            Admission::Claude(admission) => admission.unavailable().map(|reason| match reason {
                Claude::DiscoveryPending => HarnessUnavailableReason::AdmissionIncomplete,
                Claude::AdapterAbsent => HarnessUnavailableReason::AdapterAbsent,
                Claude::AdapterVersionUnreadable => {
                    HarnessUnavailableReason::AdapterVersionUnreadable
                }
                Claude::NodeAbsent => HarnessUnavailableReason::NodeAbsent,
                Claude::NodeVersionUnreadable => HarnessUnavailableReason::NodeVersionUnreadable,
                Claude::NodeEngineMismatch => HarnessUnavailableReason::NodeEngineMismatch,
                Claude::CandidateInvalid => HarnessUnavailableReason::CandidateInvalid,
                Claude::Drift => HarnessUnavailableReason::DriftDetected,
            }),
            Admission::CustomAcp(admission) => {
                admission.unavailable().map(|reason| reason.reason())
            }
            Admission::ClaudeSdk(admission) => admission.unavailable().map(Into::into),
            Admission::OpenCode(admission) => admission.unavailable().map(|reason| match reason {
                OpenCode::DiscoveryPending => HarnessUnavailableReason::AdmissionIncomplete,
                OpenCode::Absent => HarnessUnavailableReason::CandidateInvalid,
                OpenCode::CandidateInvalid => HarnessUnavailableReason::CandidateInvalid,
                OpenCode::Drift => HarnessUnavailableReason::DriftDetected,
            }),
            Admission::PiNative(admission) => admission.unavailable().map(|reason| {
                use crate::acp::bindings::pi::native_admission::PiAdmissionUnavailable as Pi;
                match reason {
                    Pi::DiscoveryPending => HarnessUnavailableReason::AdmissionIncomplete,
                    Pi::Absent | Pi::PackageUnreadable | Pi::VersionUnsupported => {
                        HarnessUnavailableReason::CandidateInvalid
                    }
                    Pi::NodeAbsent => HarnessUnavailableReason::NodeAbsent,
                    Pi::NodeVersionUnreadable => HarnessUnavailableReason::NodeVersionUnreadable,
                    Pi::NodeEngineMismatch => HarnessUnavailableReason::NodeEngineMismatch,
                    Pi::Drift => HarnessUnavailableReason::DriftDetected,
                }
            }),
        }
    }
}
