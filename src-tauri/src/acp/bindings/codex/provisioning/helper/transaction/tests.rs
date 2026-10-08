use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
use crate::acp::bindings::codex::provisioning::{AclPlan, HandoffPayload, SealedHandoff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrashPoint {
    JournalCreated,
    StateNativePublished,
    StatePhaseWritten,
    PackagePublished,
    PackagePhaseWritten,
    HandoffRemoved,
    StageRootRemoved,
}

#[derive(Debug)]
struct FakePlatform {
    journal: Option<Vec<u8>>,
    observation: NamespaceObservation,
    crash: Option<CrashPoint>,
    replace_count: usize,
    acl_applications: usize,
    fail_acl: bool,
    fail_publish: bool,
    fail_proof: bool,
    fail_cleanup: bool,
    fail_retire_state: bool,
    state_identity: NodeIdentity,
    bound_state_identity: Option<NodeIdentity>,
    state_effect_published: bool,
    stage_root_present: bool,
    handoff_present: bool,
    displace_state_during_failed_proof: bool,
    held_published_pair: bool,
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self {
            journal: None,
            observation: NamespaceObservation {
                stage_package: true,
                final_package: false,
                final_state: false,
            },
            crash: None,
            replace_count: 0,
            acl_applications: 0,
            fail_acl: false,
            fail_publish: false,
            fail_proof: false,
            fail_cleanup: false,
            fail_retire_state: false,
            state_identity: NodeIdentity {
                volume_serial: 7,
                file_id: 11,
            },
            bound_state_identity: None,
            state_effect_published: false,
            stage_root_present: true,
            handoff_present: true,
            displace_state_during_failed_proof: false,
            held_published_pair: false,
        }
    }
}

impl FakePlatform {
    fn crash_if(&mut self, point: CrashPoint) {
        if self.crash == Some(point) {
            self.crash = None;
            panic!("simulated helper process crash at {point:?}");
        }
    }
}

impl HelperTransactionPlatform for FakePlatform {
    type Proof = &'static str;

    fn read_journal(
        &mut self,
        _path: &Path,
        _byte_limit: usize,
    ) -> Result<Option<Vec<u8>>, ElevatedHelperError> {
        Ok(self.journal.clone())
    }

    fn create_journal(&mut self, _path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError> {
        if self.journal.is_some() {
            return Err(ElevatedHelperError::Collision);
        }
        self.journal = Some(bytes.to_vec());
        self.crash_if(CrashPoint::JournalCreated);
        Ok(())
    }

    fn replace_journal(&mut self, _path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError> {
        self.journal = Some(bytes.to_vec());
        self.replace_count += 1;
        let point = if self.replace_count == 1 {
            CrashPoint::StatePhaseWritten
        } else {
            CrashPoint::PackagePhaseWritten
        };
        self.crash_if(point);
        Ok(())
    }

    fn remove_journal(&mut self, _path: &Path) -> Result<(), ElevatedHelperError> {
        self.journal = None;
        Ok(())
    }

    fn observe(&mut self) -> Result<NamespaceObservation, ElevatedHelperError> {
        Ok(self.observation)
    }

    fn bind_state_identity(
        &mut self,
        expected: Option<NodeIdentity>,
    ) -> Result<NodeIdentity, ElevatedHelperError> {
        if expected.is_some_and(|expected| expected != self.state_identity) {
            return Err(ElevatedHelperError::Drift);
        }
        self.bound_state_identity = Some(self.state_identity);
        Ok(self.state_identity)
    }

    fn revalidate_stage(&mut self) -> Result<(), ElevatedHelperError> {
        self.observation
            .stage_package
            .then_some(())
            .ok_or(ElevatedHelperError::Drift)
    }

    fn revalidate_state(&mut self) -> Result<(), ElevatedHelperError> {
        (self.observation.final_state && self.bound_state_identity == Some(self.state_identity))
            .then_some(())
            .ok_or(ElevatedHelperError::Drift)
    }

    fn revalidate_stage_acl(&mut self) -> Result<(), ElevatedHelperError> {
        (self.acl_applications > 0)
            .then_some(())
            .ok_or(ElevatedHelperError::AclApplication)
    }

    fn apply_package_acl(&mut self) -> Result<(), ElevatedHelperError> {
        self.acl_applications += 1;
        if self.fail_acl {
            Err(ElevatedHelperError::AclApplication)
        } else {
            Ok(())
        }
    }

    fn publish_state_with_acl(&mut self) -> Result<(), ElevatedHelperError> {
        self.observation.final_state = true;
        self.state_effect_published = true;
        self.crash_if(CrashPoint::StateNativePublished);
        Ok(())
    }

    fn publish_package_exact(&mut self) -> Result<(), ElevatedHelperError> {
        if self.fail_publish {
            return Err(ElevatedHelperError::Publication);
        }
        self.observation.stage_package = false;
        self.observation.final_package = true;
        self.crash_if(CrashPoint::PackagePublished);
        Ok(())
    }

    fn final_managed_proof(&mut self) -> Result<Self::Proof, ElevatedHelperError> {
        if self.fail_proof {
            if self.displace_state_during_failed_proof {
                self.observation.final_state = false;
            }
            Err(ElevatedHelperError::FinalProof)
        } else {
            Ok("managed-proof")
        }
    }

    fn remove_completed_stage(&mut self) -> Result<(), ElevatedHelperError> {
        if self.fail_cleanup {
            return Err(ElevatedHelperError::Cleanup);
        }
        if self.handoff_present {
            self.handoff_present = false;
            self.crash_if(CrashPoint::HandoffRemoved);
        }
        if self.stage_root_present {
            self.stage_root_present = false;
            self.crash_if(CrashPoint::StageRootRemoved);
        }
        self.held_published_pair = false;
        Ok(())
    }

    fn rollback_package(&mut self) -> Result<(), ElevatedHelperError> {
        self.observation.final_package = false;
        self.observation.stage_package = true;
        Ok(())
    }

    fn rollback_state(&mut self) -> Result<(), ElevatedHelperError> {
        if self.bound_state_identity != Some(self.state_identity) {
            return Err(ElevatedHelperError::Drift);
        }
        if self.state_effect_published {
            self.observation.final_state = false;
            self.state_effect_published = false;
        }
        Ok(())
    }

    fn hold_published_pair(&mut self) -> Result<(), ElevatedHelperError> {
        if !self.observation.final_package || !self.observation.final_state {
            return Err(ElevatedHelperError::Drift);
        }
        self.observation.final_package = false;
        self.observation.final_state = false;
        self.state_effect_published = false;
        self.held_published_pair = true;
        Ok(())
    }

    fn discard_held_pair(&mut self) -> Result<(), ElevatedHelperError> {
        (!self.held_published_pair)
            .then_some(())
            .ok_or(ElevatedHelperError::Cleanup)
    }

    fn restore_held_pair(&mut self) -> Result<(), ElevatedHelperError> {
        self.held_published_pair
            .then_some(())
            .ok_or(ElevatedHelperError::Rollback)?;
        self.observation.final_package = true;
        self.observation.final_state = true;
        self.held_published_pair = false;
        Ok(())
    }

    fn retire_published_package(&mut self) -> Result<(), ElevatedHelperError> {
        self.observation.final_package = false;
        Ok(())
    }

    fn retire_published_state(&mut self) -> Result<(), ElevatedHelperError> {
        if self.fail_retire_state {
            return Err(ElevatedHelperError::Cleanup);
        }
        self.observation.final_state = false;
        Ok(())
    }
}

fn request(nonce: &str) -> AuthenticatedHelperRequest {
    let package_target = PathBuf::from(r"C:\SageHarnesses\Codex\0.146.0\package");
    let payload = HandoffPayload {
        schema_version: 1,
        binding_id: "codex-app-server".into(),
        runtime_version: "0.146.0".into(),
        creation_nonce: nonce.into(),
        source_proof_sha256: "11".repeat(32),
        aggregate_byte_len: 429_289_628,
        files: vec![],
        package_target: package_target.clone(),
        state_target: PathBuf::from(
            r"C:\SageHarnesses\CodexState\codex-app-server-0.146.0-d3992fec",
        ),
        current_user_sid_hex: "010100000000000512000000".into(),
        package_acl: acl("builtinAdministrators", 0x0012_00a9),
        state_acl: acl("currentUser", 0x001f_01ff),
        acl_application_status: "requiredPrivilegedHelperNotApplied".into(),
        publish_status: "notPublished".into(),
    };
    AuthenticatedHelperRequest {
        payload: super::super::HelperRequestPayload {
            schema_version: 1,
            operation: super::super::HelperOperation::Publish,
            stage_root: package_target
                .parent()
                .unwrap()
                .join(".sage-codex-0.146.0-stage"),
            helper: super::super::HelperExecutableIdentity {
                product: "sage-codex-0146-elevated-helper".into(),
                protocol_version: 1,
                sage_version: "0.1.0".into(),
                executable_sha256: "22".repeat(32),
                executable_byte_len: 1,
            },
            handoff: SealedHandoff {
                payload,
                payload_sha256: "33".repeat(32),
            },
        },
        journal_key: vec![nonce.as_bytes().first().copied().unwrap_or(0); 32],
    }
}

fn acl(owner: &str, current_user_access_mask: u32) -> AclPlan {
    AclPlan {
        owner: owner.into(),
        protected_dacl: true,
        system_access_mask: 0x001f_01ff,
        administrators_access_mask: 0x001f_01ff,
        current_user_access_mask,
        inheritance_flags: 3,
    }
}

#[test]
fn exact_transaction_publishes_and_removes_the_journal() {
    let mut platform = FakePlatform::default();
    assert_eq!(finalize(&request("00"), &mut platform), Ok("managed-proof"));
    assert_eq!(platform.journal, None);
    assert_eq!(
        platform.observation,
        NamespaceObservation {
            stage_package: false,
            final_package: true,
            final_state: true,
        }
    );
}

#[test]
fn every_effect_and_phase_boundary_recovers_after_process_crash() {
    for point in [
        CrashPoint::JournalCreated,
        CrashPoint::StateNativePublished,
        CrashPoint::StatePhaseWritten,
        CrashPoint::PackagePublished,
        CrashPoint::PackagePhaseWritten,
        CrashPoint::HandoffRemoved,
        CrashPoint::StageRootRemoved,
    ] {
        let request = request("00");
        let mut platform = FakePlatform {
            crash: Some(point),
            ..FakePlatform::default()
        };
        assert!(catch_unwind(AssertUnwindSafe(|| finalize(&request, &mut platform))).is_err());
        assert_eq!(
            finalize(&request, &mut platform),
            Ok("managed-proof"),
            "{point:?}"
        );
    }
}

#[test]
fn rollback_follows_the_retained_state_effect_when_its_canonical_name_disappears() {
    let mut platform = FakePlatform {
        fail_proof: true,
        displace_state_during_failed_proof: true,
        ..FakePlatform::default()
    };

    assert_eq!(
        finalize(&request("00"), &mut platform),
        Err(ElevatedHelperError::FinalProof)
    );
    assert!(!platform.state_effect_published);
    assert_eq!(platform.journal, None);
}

#[test]
fn recovery_rejects_a_same_user_state_swap_after_native_publication() {
    let request = request("00");
    let mut platform = FakePlatform {
        crash: Some(CrashPoint::StateNativePublished),
        ..FakePlatform::default()
    };
    let _ = catch_unwind(AssertUnwindSafe(|| finalize(&request, &mut platform)));
    platform.state_identity.file_id += 1;

    assert_eq!(
        finalize(&request, &mut platform),
        Err(ElevatedHelperError::Drift)
    );
    assert!(platform.observation.final_state);
}

#[test]
fn invalid_journal_sizes_authentication_and_replay_are_rejected() {
    let request = request("00");
    for bytes in [vec![b'x'; JOURNAL_BYTES_MAX + 1], vec![b'{']] {
        let mut platform = FakePlatform {
            journal: Some(bytes),
            ..FakePlatform::default()
        };
        assert_eq!(
            finalize(&request, &mut platform),
            Err(ElevatedHelperError::Journal)
        );
    }

    let mut platform = FakePlatform {
        crash: Some(CrashPoint::JournalCreated),
        ..FakePlatform::default()
    };
    let _ = catch_unwind(AssertUnwindSafe(|| finalize(&request, &mut platform)));
    let mut exact = platform.journal.clone().unwrap();
    exact.resize(JOURNAL_BYTES_MAX, b' ');
    let mut exact_platform = FakePlatform {
        journal: Some(exact),
        ..FakePlatform::default()
    };
    assert_eq!(finalize(&request, &mut exact_platform), Ok("managed-proof"));

    let mut forged: SealedJournal =
        serde_json::from_slice(platform.journal.as_ref().unwrap()).unwrap();
    let replacement = if forged.authenticator_sha256.starts_with('f') {
        "e"
    } else {
        "f"
    };
    forged.authenticator_sha256.replace_range(0..1, replacement);
    platform.journal = Some(serde_json::to_vec(&forged).unwrap());
    assert_eq!(
        finalize(&request, &mut platform),
        Err(ElevatedHelperError::Journal)
    );

    let mut completed = FakePlatform::default();
    assert!(finalize(&request, &mut completed).is_ok());
    assert_eq!(
        finalize(&request, &mut completed),
        Err(ElevatedHelperError::Collision)
    );
}

#[test]
fn nonce_drift_cannot_claim_an_authenticated_recovery_journal() {
    let original = request("00");
    let mut platform = FakePlatform {
        crash: Some(CrashPoint::JournalCreated),
        ..FakePlatform::default()
    };
    let _ = catch_unwind(AssertUnwindSafe(|| finalize(&original, &mut platform)));
    assert_eq!(
        finalize(&request("11"), &mut platform),
        Err(ElevatedHelperError::Journal)
    );
}

#[test]
fn acl_publish_and_final_proof_failures_roll_back_both_roots() {
    for (expected, mut platform) in [
        (
            ElevatedHelperError::AclApplication,
            FakePlatform {
                fail_acl: true,
                ..FakePlatform::default()
            },
        ),
        (
            ElevatedHelperError::Publication,
            FakePlatform {
                fail_publish: true,
                ..FakePlatform::default()
            },
        ),
        (
            ElevatedHelperError::FinalProof,
            FakePlatform {
                fail_proof: true,
                ..FakePlatform::default()
            },
        ),
    ] {
        assert_eq!(finalize(&request("00"), &mut platform), Err(expected));
        assert_eq!(platform.journal, None);
        assert_eq!(
            platform.observation,
            NamespaceObservation {
                stage_package: true,
                final_package: false,
                final_state: false,
            }
        );
    }
}

#[test]
fn foreign_target_collision_is_never_rolled_back() {
    let mut platform = FakePlatform {
        observation: NamespaceObservation {
            stage_package: true,
            final_package: false,
            final_state: true,
        },
        ..FakePlatform::default()
    };
    assert_eq!(
        finalize(&request("00"), &mut platform),
        Err(ElevatedHelperError::Collision)
    );
    assert!(platform.observation.final_state);
}

#[test]
fn post_commit_cleanup_failure_retains_journal_and_never_rolls_back() {
    let request = request("00");
    let mut platform = FakePlatform {
        fail_cleanup: true,
        ..FakePlatform::default()
    };
    assert_eq!(
        finalize(&request, &mut platform),
        Err(ElevatedHelperError::Cleanup)
    );
    assert!(platform.journal.is_some());
    assert!(platform.observation.final_package);
    assert!(platform.observation.final_state);

    platform.fail_cleanup = false;
    assert_eq!(finalize(&request, &mut platform), Ok("managed-proof"));
}

#[test]
fn interrupted_publish_resumes_from_the_journal() {
    let original = request("00");
    let mut platform = FakePlatform {
        crash: Some(CrashPoint::StatePhaseWritten),
        ..FakePlatform::default()
    };

    let _ = catch_unwind(AssertUnwindSafe(|| finalize(&original, &mut platform)));
    drop(original);

    assert_eq!(
        execute_boundary_recover(&request("00"), &mut platform),
        Ok("managed-proof")
    );
    assert_eq!(platform.journal, None);
}

#[test]
fn recovery_required_retries_cleanup_without_republishing() {
    let original = request("00");
    let mut platform = FakePlatform {
        fail_cleanup: true,
        ..FakePlatform::default()
    };
    assert_eq!(
        finalize(&original, &mut platform),
        Err(ElevatedHelperError::Cleanup)
    );
    drop(original);

    platform.fail_cleanup = false;
    assert_eq!(
        execute_boundary_recover(&request("00"), &mut platform),
        Ok("managed-proof")
    );
    assert_eq!(platform.acl_applications, 1);
    assert_eq!(platform.journal, None);
}

#[test]
fn update_replaces_an_existing_canonical_target() {
    let mut platform = FakePlatform {
        observation: NamespaceObservation {
            stage_package: true,
            final_package: true,
            final_state: true,
        },
        ..FakePlatform::default()
    };

    assert_eq!(
        execute_boundary_update(&request("00"), &mut platform),
        Ok("managed-proof")
    );
    assert_eq!(platform.journal, None);
    assert_eq!(
        platform.observation,
        NamespaceObservation {
            stage_package: false,
            final_package: true,
            final_state: true,
        }
    );
}

#[test]
fn rollback_converges_after_failed_update() {
    let mut platform = FakePlatform {
        observation: NamespaceObservation {
            stage_package: true,
            final_package: true,
            final_state: true,
        },
        fail_publish: true,
        ..FakePlatform::default()
    };

    assert_eq!(
        execute_boundary_update(&request("00"), &mut platform),
        Err(ElevatedHelperError::Publication)
    );
    assert!(platform.observation.final_package);
    assert!(platform.observation.final_state);
    assert!(platform.observation.stage_package);
    assert_eq!(platform.journal, None);
}

#[test]
fn update_cleanup_retains_the_new_pair_and_journal_for_recovery() {
    let request = request("00");
    let mut platform = FakePlatform {
        observation: NamespaceObservation {
            stage_package: true,
            final_package: true,
            final_state: true,
        },
        fail_cleanup: true,
        ..FakePlatform::default()
    };

    assert_eq!(
        execute_boundary_update(&request, &mut platform),
        Err(ElevatedHelperError::Cleanup)
    );
    assert!(platform.journal.is_some());
    assert!(platform.observation.final_package);
    assert!(platform.observation.final_state);
    assert!(platform.held_published_pair);

    platform.fail_cleanup = false;
    assert_eq!(
        execute_boundary_recover(&request, &mut platform),
        Ok("managed-proof")
    );
    assert_eq!(platform.journal, None);
    assert!(!platform.held_published_pair);
}

#[test]
fn remove_retires_published_package_and_state() {
    let mut platform = FakePlatform {
        observation: NamespaceObservation {
            stage_package: false,
            final_package: true,
            final_state: true,
        },
        ..FakePlatform::default()
    };

    assert_eq!(
        execute_boundary_remove(&request("00"), &mut platform),
        Ok(())
    );
    assert_eq!(platform.journal, None);
    assert_eq!(
        platform.observation,
        NamespaceObservation {
            stage_package: false,
            final_package: false,
            final_state: false,
        }
    );
}

#[test]
fn remove_keeps_a_journal_when_state_retirement_fails_after_package_retirement() {
    let request = request("00");
    let mut platform = FakePlatform {
        observation: NamespaceObservation {
            stage_package: false,
            final_package: true,
            final_state: true,
        },
        fail_retire_state: true,
        ..FakePlatform::default()
    };

    assert_eq!(
        execute_boundary_remove(&request, &mut platform),
        Err(ElevatedHelperError::Cleanup)
    );
    assert!(platform.journal.is_some());
    assert!(!platform.observation.final_package);
    assert!(platform.observation.final_state);

    platform.fail_retire_state = false;
    assert_eq!(execute_boundary_remove(&request, &mut platform), Ok(()));
    assert_eq!(platform.journal, None);
    assert!(!platform.observation.final_package);
    assert!(!platform.observation.final_state);
}
