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
    pub version: Option<String>,
    pub version_checking: bool,
    pub bound_version: Option<String>,
    pub characterisation: Option<HarnessCharacterisation>,
    pub unavailable_reason: Option<HarnessUnavailableReason>,
    pub sign_in: Option<HarnessSignIn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HarnessInstallState {
    Detecting,
    Available,
    Unavailable,
    NotInstalled,
}

impl HarnessInstallState {
    pub(crate) const fn as_str(self) -> &'static str {
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
    Characterised,
    Uncharacterised,
}

impl HarnessCharacterisation {
    pub(crate) fn is_characterised(self) -> bool {
        matches!(self, Self::Characterised)
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
    CandidateInvalid,
    CapabilityProjectionUnavailable,
    NoNativeAcpEndpoint,
    AcpProbeUncharacterised,
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
}
