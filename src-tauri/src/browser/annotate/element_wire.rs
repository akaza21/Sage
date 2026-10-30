use super::*;
use serde::{Deserializer, Serializer, de, ser::SerializeMap};

impl Serialize for BrowserElement {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("target", &self.target)?;
        map.serialize_entry("ordinal", &self.ordinal)?;
        map.serialize_entry("tabId", &self.tab_id)?;
        map.serialize_entry("url", &self.url)?;
        map.serialize_entry("title", &self.title)?;
        if self.target == Target::Element {
            map.serialize_entry("selector", &self.selector)?;
            map.serialize_entry("role", &self.role)?;
            map.serialize_entry("name", &self.name)?;
        }
        map.serialize_entry("text", &self.text)?;
        map.serialize_entry("box", &self.bounds)?;
        map.serialize_entry("comment", &self.comment)?;
        map.end()
    }
}

/// Decoding checks the bounds only. Stored turns and queued prompts decode
/// through here too, and a later, stricter redaction rule must not make them
/// unreadable; the redaction fixpoints run where a prompt is admitted
/// (`attached_context::admit_browser_elements`).
impl<'de> Deserialize<'de> for BrowserElement {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            kind: Kind,
            target: Target,
            ordinal: u8,
            tab_id: String,
            url: String,
            title: String,
            #[serde(default, deserialize_with = "present_string")]
            selector: Option<String>,
            #[serde(default, deserialize_with = "present_string")]
            role: Option<String>,
            #[serde(default, deserialize_with = "present_string")]
            name: Option<String>,
            text: String,
            #[serde(rename = "box")]
            bounds: ElementBox,
            comment: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        let element_fields = wire.selector.is_some() && wire.role.is_some() && wire.name.is_some();
        let area_fields = wire.selector.is_none() && wire.role.is_none() && wire.name.is_none();
        if !(wire.target == Target::Element && element_fields
            || wire.target == Target::Area && area_fields)
        {
            return Err(de::Error::custom("browser element target fields rejected"));
        }
        let element = BrowserElement {
            kind: wire.kind,
            target: wire.target,
            ordinal: wire.ordinal,
            tab_id: wire.tab_id,
            url: wire.url,
            title: wire.title,
            selector: wire.selector.unwrap_or_default(),
            role: wire.role.unwrap_or_default(),
            name: wire.name.unwrap_or_default(),
            text: wire.text,
            bounds: wire.bounds,
            comment: wire.comment,
        };
        validate_bounds(&element).map_err(de::Error::custom)?;
        Ok(element)
    }
}

fn present_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}
