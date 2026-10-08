use super::*;

impl CodexBindingRegistration {
    #[cfg(test)]
    pub(crate) fn unavailable_for_test() -> Self {
        Self {
            authority: Arc::new(CodexRegistrationAuthority {
                state: Mutex::new(CodexRegistrationState::Unavailable(
                    CodexAdmissionUnavailable::IndependentAdmission,
                )),
                #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
                fixture_only: false,
            }),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn unsigned_for_test() -> Self {
        Self::unavailable_for_test()
    }

    #[cfg(all(test, windows))]
    pub(crate) fn observation_only_for_test() -> Self {
        let registration = Self::unavailable_for_test();
        assert_eq!(
            registration.refuse_independent_launch_after_workstation_authenticode_observation(),
            CodexAdmissionUnavailable::ObservationOnly
        );
        registration
    }

    #[cfg(all(test, windows))]
    pub(crate) fn admitted_for_test() -> Self {
        Self {
            authority: Arc::new(CodexRegistrationAuthority {
                state: Mutex::new(CodexRegistrationState::Admitted(Arc::new(
                    authenticate_tests::admitted_binding_fixture(),
                ))),
                #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
                fixture_only: false,
            }),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn admitted_for_journey_test(case_ids: &'static [&'static str]) -> Self {
        Self {
            authority: Arc::new(CodexRegistrationAuthority {
                state: Mutex::new(CodexRegistrationState::Admitted(Arc::new(
                    authenticate_tests::admitted_journey_binding_fixture(case_ids),
                ))),
                #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
                fixture_only: false,
            }),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn admitted_for_observed_steer_journey_test(
        case_ids: &'static [&'static str],
    ) -> Self {
        Self {
            authority: Arc::new(CodexRegistrationAuthority {
                state: Mutex::new(CodexRegistrationState::Admitted(Arc::new(
                    authenticate_tests::admitted_observed_steer_journey_fixture(case_ids),
                ))),
                #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
                fixture_only: false,
            }),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn induce_drift_for_test(&self) {
        let binding = match &*self.lock_state() {
            CodexRegistrationState::Admitted(binding) => Arc::clone(binding),
            CodexRegistrationState::Unavailable(_) => panic!("registration already unavailable"),
        };
        std::fs::write(
            binding
                .packet()
                .codex_home()
                .join("sessions")
                .join("registration-drift"),
            b"drift",
        )
        .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn same_authority(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.authority, &other.authority)
    }
}
