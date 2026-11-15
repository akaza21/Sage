use serde::{Deserialize, Deserializer, Serialize};

/// What an error item is when it is not the Turn's own failure. An item
/// without a kind is a failure the harness or Sage reported for its Turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// A failed attempt the harness retries on its own.
    Attempt,
    /// Information shown on the Turn, such as a capability this session
    /// lacks or a single refused request; the Turn goes on.
    Notice,
}

pub(super) fn deserialize_optional<'de, D>(deserializer: D) -> Result<Option<ErrorKind>, D::Error>
where
    D: Deserializer<'de>,
{
    let kind = Option::<String>::deserialize(deserializer)?;
    Ok(kind.as_deref().and_then(|kind| match kind {
        "attempt" => Some(ErrorKind::Attempt),
        "notice" => Some(ErrorKind::Notice),
        _ => None,
    }))
}
