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
}
