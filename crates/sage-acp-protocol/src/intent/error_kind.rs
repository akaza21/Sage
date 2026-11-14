use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    Attempt,
}

pub(super) fn deserialize_optional<'de, D>(deserializer: D) -> Result<Option<ErrorKind>, D::Error>
where
    D: Deserializer<'de>,
{
    let kind = Option::<String>::deserialize(deserializer)?;
    Ok(kind.as_deref().and_then(|kind| match kind {
        "attempt" => Some(ErrorKind::Attempt),
        _ => None,
    }))
}
