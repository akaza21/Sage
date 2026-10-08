use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::image_attachment::MAX_OUTGOING_IMAGE_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PromptContentKind {
    Image,
    File,
    Document,
    Audio,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PromptContentLimit {
    pub(crate) max_items: u8,
    pub(crate) max_bytes_each: u32,
    pub(crate) max_bytes_total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionPromptContentV1 {
    pub(crate) schema_version: u8,
    pub(crate) kinds: BTreeMap<PromptContentKind, PromptContentLimit>,
}

impl Default for SessionPromptContentV1 {
    fn default() -> Self {
        Self {
            schema_version: 1,
            kinds: BTreeMap::new(),
        }
    }
}

impl SessionPromptContentV1 {
    pub(crate) fn from_acp_initialize(response: &Value) -> Self {
        if response
            .pointer("/result/agentCapabilities/promptCapabilities/image")
            .and_then(Value::as_bool)
            == Some(true)
        {
            Self::with_image()
        } else {
            Self::default()
        }
    }

    pub(crate) fn with_image() -> Self {
        let mut value = Self::default();
        value.kinds.insert(
            PromptContentKind::Image,
            PromptContentLimit {
                max_items: 4,
                max_bytes_each: MAX_OUTGOING_IMAGE_BYTES,
                max_bytes_total: 4 * MAX_OUTGOING_IMAGE_BYTES,
            },
        );
        value
    }

    #[allow(dead_code)] // The native send gate follows in the egress slice.
    pub(crate) fn permits(&self, kind: PromptContentKind) -> bool {
        self.validate()
            && self.kinds.get(&kind).is_some_and(|limit| {
                limit.max_items > 0 && limit.max_bytes_each > 0 && limit.max_bytes_total > 0
            })
    }

    pub(crate) fn validate(&self) -> bool {
        if self.schema_version != 1 {
            return false;
        }
        self.kinds.iter().all(|(kind, limit)| match kind {
            PromptContentKind::Image => {
                limit.max_items <= 4
                    && limit.max_bytes_each <= MAX_OUTGOING_IMAGE_BYTES
                    && limit.max_bytes_total <= 4 * MAX_OUTGOING_IMAGE_BYTES
                    && limit.max_items > 0
                    && limit.max_bytes_each > 0
                    && limit.max_bytes_total > 0
            }
            _ => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_and_unknown_content_are_not_capabilities() {
        let empty = SessionPromptContentV1::default();
        assert!(!empty.permits(PromptContentKind::Image));
        assert!(empty.validate());
        let image = SessionPromptContentV1::with_image();
        assert!(image.permits(PromptContentKind::Image));
        assert!(!image.permits(PromptContentKind::File));
        assert!(image.validate());
    }

    #[test]
    fn content_limits_cannot_exceed_sage_policy() {
        let mut value = SessionPromptContentV1::with_image();
        value
            .kinds
            .get_mut(&PromptContentKind::Image)
            .unwrap()
            .max_bytes_each += 1;
        assert!(!value.validate());
    }

    #[test]
    fn acp_image_requires_exact_true_in_initialize_response() {
        let offer = json!({"result":{"agentCapabilities":{"promptCapabilities":{"image":true}}}});
        assert!(
            SessionPromptContentV1::from_acp_initialize(&offer).permits(PromptContentKind::Image)
        );
        for value in [
            json!({"result":{"agentCapabilities":{}}}),
            json!({"result":{"agentCapabilities":{"promptCapabilities":{"image":false}}}}),
            json!({"result":{"agentCapabilities":{"promptCapabilities":{"image":"true"}}}}),
            json!({"result":{"agentCapabilities":{"promptCapabilities":null}}}),
        ] {
            assert!(
                !SessionPromptContentV1::from_acp_initialize(&value)
                    .permits(PromptContentKind::Image)
            );
        }
    }
}
