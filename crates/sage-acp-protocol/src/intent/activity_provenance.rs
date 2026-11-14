//! Where an activity item came from, as the harness labelled it, checked so
//! no absolute path reaches persistence.

use serde::{Deserialize, Serialize};

use crate::contract::{ContractError, RequiredText, SchemaVersionV1};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivitySourceKind {
    Tool,
    Extension,
    Collaboration,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityProvenanceV1 {
    pub schema_version: SchemaVersionV1,
    pub source_kind: ActivitySourceKind,
    pub external_kind: RequiredText<128>,
    pub extension_schema_id: Option<RequiredText<128>>,
    pub safe_label: Option<RequiredText<512>>,
}

impl ActivityProvenanceV1 {
    pub fn validate_for_persistence(&self) -> Result<(), ContractError> {
        let fields = [
            self.external_kind.as_str(),
            self.extension_schema_id
                .as_ref()
                .map_or("", RequiredText::as_str),
            self.safe_label.as_ref().map_or("", RequiredText::as_str),
        ];
        if fields.into_iter().any(contains_absolute_path_authority) {
            return Err(ContractError::ForbiddenContent {
                field: "activityProvenance",
            });
        }
        Ok(())
    }
}

fn contains_absolute_path_authority(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value.as_bytes().get(1) == Some(&b':')
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
            && value
                .as_bytes()
                .get(2)
                .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
}
