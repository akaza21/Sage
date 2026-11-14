use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub install_state: String,
    pub source: String,
    pub transport: HarnessTransport,
    pub binding_id: Option<String>,
    pub account_model_discovery: bool,
    #[serde(default)]
    pub access_modes: Vec<sage_acp_protocol::access_mode::AccessModeCapability>,
    pub version: Option<String>,
    pub version_checking: bool,
    pub bound_version: Option<String>,
    pub characterisation: Option<HarnessCharacterisation>,
    pub unavailable_reason: Option<HarnessUnavailableReason>,
    pub sign_in: Option<HarnessSignIn>,
    /// The brand mark of a curated harness; `None` for any other harness.
    #[serde(default)]
    pub brand: Option<HarnessBrand>,
    /// A newer published release of the installed CLI, from the last check.
    #[serde(default)]
    pub update: Option<HarnessUpdate>,
}

/// A newer published release and the command that installs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessUpdate {
    pub latest_version: String,
    pub command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessInstallState {
    Detecting,
    Available,
    Unavailable,
    NotInstalled,
}

impl HarnessInstallState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Detecting => "detecting",
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::NotInstalled => "notInstalled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HarnessCharacterisation {
    /// The release Sage was tested with.
    Characterised,
    /// A later release. It keeps the tested features; the chat notes that it
    /// is newer than tested.
    NewerThanTested,
    /// An earlier release, or one whose version cannot be read.
    Uncharacterised,
}

impl HarnessCharacterisation {
    /// Whether the features Sage tested apply to this release.
    pub fn runs_tested_features(self) -> bool {
        matches!(self, Self::Characterised | Self::NewerThanTested)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HarnessUnavailableReason {
    PlatformUnsupported,
    ObservationOnly,
    AdmissionIncomplete,
    DriftDetected,
    AdapterAbsent,
    AdapterVersionUnreadable,
    NodeAbsent,
    NodeVersionUnreadable,
    NodeEngineMismatch,
    /// The installed release predates the oldest one whose protocol Sage
    /// speaks; updating it fixes this.
    VersionTooOld,
    CandidateInvalid,
    CapabilityProjectionUnavailable,
    NoNativeAcpEndpoint,
    AcpProbeUncharacterised,
}

/// Brand identity is curated install metadata, never inferred from a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HarnessBrand {
    Claude,
    Codex,
    OpenCode,
    Pi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessSignIn {
    pub command: String,
    pub hint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HarnessTransport {
    Acp,
    LegacySimulator,
}

impl HarnessDefinition {
    pub fn validate_transport_contract(&self) -> Result<(), &'static str> {
        if self.account_model_discovery
            && (self.transport != HarnessTransport::Acp || self.binding_id.is_none())
        {
            return Err("account model discovery requires an ACP binding");
        }
        match (self.transport, self.binding_id.as_deref()) {
            (HarnessTransport::LegacySimulator, None) => Ok(()),
            (HarnessTransport::LegacySimulator, Some(_)) => {
                Err("legacy simulator cannot declare an ACP binding")
            }
            (HarnessTransport::Acp, None) if self.install_state != "available" => Ok(()),
            (HarnessTransport::Acp, None) => Err("an available ACP harness requires a binding ID"),
            (HarnessTransport::Acp, Some(binding_id))
                if !binding_id.is_empty()
                    && binding_id.len() <= 512
                    && !binding_id.contains('\0') =>
            {
                Ok(())
            }
            (HarnessTransport::Acp, Some(_)) => Err("ACP binding ID is invalid"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub is_default: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    /// The thinking levels this model takes, when the harness lists them
    /// before a session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_levels: Option<Vec<String>>,
}

impl ModelOption {
    /// `model`, or the listed default when none is named.
    pub fn launched<'a>(models: &'a [Self], model: Option<&str>) -> Option<&'a Self> {
        models.iter().find(|entry| match model {
            Some(model) => entry.id == model,
            None => entry.is_default == Some(true),
        })
    }

    pub fn lists_effort(&self, effort: &str) -> bool {
        self.effort_levels
            .as_ref()
            .is_some_and(|levels| levels.iter().any(|level| level == effort))
    }
}
