use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCategory {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Fetch,
    WebSearch,
    ImageView,
    Think,
    SwitchMode,
    Mcp,
    Subagent,
    Other,
}

impl ToolCategory {
    pub fn from_wire(value: &str) -> Self {
        match value {
            "read" | "file" => Self::Read,
            "edit" | "diff" => Self::Edit,
            "delete" => Self::Delete,
            "move" => Self::Move,
            "search" => Self::Search,
            "execute" | "command" => Self::Execute,
            "fetch" | "web" => Self::Fetch,
            "webSearch" => Self::WebSearch,
            "imageView" => Self::ImageView,
            "think" => Self::Think,
            "switchMode" | "switch_mode" => Self::SwitchMode,
            "mcp" => Self::Mcp,
            "subagent" => Self::Subagent,
            _ => Self::Other,
        }
    }
}

impl<'de> Deserialize<'de> for ToolCategory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = std::borrow::Cow::<'de, str>::deserialize(deserializer)?;
        Ok(Self::from_wire(value.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::ToolCategory;

    fn round_trip(stored: &str) -> (ToolCategory, String) {
        let decoded: ToolCategory =
            serde_json::from_value(serde_json::json!(stored)).expect("decode");
        let encoded = serde_json::to_value(decoded).expect("encode");
        (decoded, encoded.as_str().expect("string").to_owned())
    }

    #[test]
    fn every_legacy_category_decodes_and_reserializes_under_its_new_name() {
        for (stored, expected, wire) in [
            ("file", ToolCategory::Read, "read"),
            ("diff", ToolCategory::Edit, "edit"),
            ("command", ToolCategory::Execute, "execute"),
            ("web", ToolCategory::Fetch, "fetch"),
            ("generic", ToolCategory::Other, "other"),
            ("search", ToolCategory::Search, "search"),
            ("mcp", ToolCategory::Mcp, "mcp"),
            ("subagent", ToolCategory::Subagent, "subagent"),
            ("other", ToolCategory::Other, "other"),
        ] {
            let (decoded, encoded) = round_trip(stored);
            assert_eq!(decoded, expected, "{stored} decodes to its new variant");
            assert_eq!(encoded, wire, "{stored} reserializes under the new name");
        }
    }

    #[test]
    fn every_new_category_name_round_trips_unchanged() {
        for wire in [
            "read",
            "edit",
            "delete",
            "move",
            "search",
            "execute",
            "fetch",
            "webSearch",
            "imageView",
            "think",
            "switchMode",
            "mcp",
            "subagent",
            "other",
        ] {
            assert_eq!(round_trip(wire).1, wire);
        }
    }

    #[test]
    fn an_unknown_category_decodes_to_other_instead_of_failing_the_page() {
        for stored in ["", "Read", "shellExecute", "\u{1f600}", "null-ish"] {
            assert_eq!(round_trip(stored).0, ToolCategory::Other, "{stored}");
        }
    }
}
