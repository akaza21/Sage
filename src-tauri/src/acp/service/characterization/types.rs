use std::path::{Path, PathBuf};

use crate::acp::{
    characterization_observation::CharacterizationObservation,
    intent::BindingId,
    process::{ProcessIdentityReceipt, SpawnTimingReceipt},
    scheduler::RuntimeOwner,
};

use super::super::AcpService;
use super::receipt::CharacterizationReceiptSink;

pub(super) const BINDING_DIGEST: [u8; 32] = [7; 32];
pub(crate) const BINDING_DIGEST_HEX: &str =
    "0707070707070707070707070707070707070707070707070707070707070707";
pub(crate) const BINDING_ID: &str = "fixture-characterization";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharacterizationCase {
    TurnSuccess,
    TurnCancelled,
    ColdCycle,
}

impl CharacterizationCase {
    pub(crate) fn parse(value: &str) -> Result<Self, CharacterizationError> {
        match value {
            "turn-success" => Ok(Self::TurnSuccess),
            "turn-cancelled" => Ok(Self::TurnCancelled),
            "cold-cycle" => Ok(Self::ColdCycle),
            _ => Err(CharacterizationError::Artifact),
        }
    }

    pub(super) fn argument(self) -> &'static str {
        match self {
            Self::TurnSuccess => "characterization-turn-success",
            Self::TurnCancelled => "characterization-turn-cancelled",
            Self::ColdCycle => "characterization-cold-cycle",
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::TurnSuccess => "turn-success",
            Self::TurnCancelled => "turn-cancelled",
            Self::ColdCycle => "cold-cycle",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CharacterizationArtifact {
    pub(super) path: PathBuf,
    pub(super) sha256: [u8; 32],
    pub(super) bytes: u64,
}

impl CharacterizationArtifact {
    pub(crate) fn new(
        path: PathBuf,
        sha256: [u8; 32],
        bytes: u64,
    ) -> Result<Self, CharacterizationError> {
        let path = path
            .canonicalize()
            .map_err(|_| CharacterizationError::Artifact)?;
        let actual = path
            .metadata()
            .map_err(|_| CharacterizationError::Artifact)?;
        if !actual.is_file() || bytes == 0 || actual.len() != bytes || sha256 == [0; 32] {
            return Err(CharacterizationError::Artifact);
        }
        Ok(Self {
            path,
            sha256,
            bytes,
        })
    }

    pub(super) fn digest_hex(&self) -> String {
        hex(&self.sha256)
    }
}

#[derive(Clone)]
pub(crate) struct CharacterizationBinding {
    pub(super) artifact: CharacterizationArtifact,
    pub(super) case: CharacterizationCase,
    receipt: CharacterizationReceiptSink,
}

impl CharacterizationBinding {
    pub(crate) fn new(
        artifact: CharacterizationArtifact,
        case: CharacterizationCase,
        receipt_root: &Path,
    ) -> Result<Self, CharacterizationError> {
        Ok(Self {
            artifact,
            case,
            receipt: CharacterizationReceiptSink::new(receipt_root, case)
                .map_err(|_| CharacterizationError::Observation)?,
        })
    }

    pub(crate) fn accepts(&self, binding_id: &BindingId) -> bool {
        binding_id.as_str() == BINDING_ID
    }

    pub(crate) fn artifact_digest(&self) -> String {
        self.artifact.digest_hex()
    }

    pub(super) fn begin_receipt(&self, owner: &RuntimeOwner) -> Result<(), CharacterizationError> {
        self.receipt
            .begin(owner)
            .map_err(|_| CharacterizationError::Observation)
    }

    pub(super) fn record_started(
        &self,
        owner: &RuntimeOwner,
        process: ProcessIdentityReceipt,
        timing: SpawnTimingReceipt,
    ) -> Result<(), CharacterizationError> {
        self.receipt
            .record_started(owner, process, timing)
            .map_err(|_| CharacterizationError::Observation)
    }

    pub(super) fn observation(&self) -> CharacterizationObservation {
        self.receipt.observation()
    }

    pub(super) fn complete_receipt(
        &self,
        owner: &RuntimeOwner,
        wal: crate::persistence::WalConvergenceReceipt,
    ) -> Result<(), CharacterizationError> {
        self.receipt
            .complete(owner, wal)
            .map_err(|_| CharacterizationError::Observation)
    }
}

impl AcpService {
    pub(crate) fn accepts_characterization(&self, binding_id: &BindingId) -> bool {
        self.characterization
            .as_ref()
            .is_some_and(|binding| binding.accepts(binding_id))
    }

    pub(crate) fn accepts_characterization_new_session(&self, binding_id: &BindingId) -> bool {
        self.accepts_characterization(binding_id)
    }

    pub(crate) fn characterization_artifact_digest(&self) -> Option<String> {
        self.characterization
            .as_ref()
            .map(CharacterizationBinding::artifact_digest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CharacterizationError {
    #[error("characterization artifact identity is invalid")]
    Artifact,
    #[error("characterization intent is stale or foreign")]
    Intent,
    #[error("characterization runtime capacity is unavailable")]
    Capacity,
    #[error("characterization durable session mutation failed")]
    Session,
    #[error("characterization process launch failed")]
    Launch,
    #[error("characterization process convergence requires recovery")]
    ProcessRecovery,
    #[error("characterization activation failed")]
    Activation,
    #[error("characterization runtime slot requires recovery")]
    Recovery,
    #[error("characterization observation receipt failed")]
    Observation,
}

pub(super) fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
