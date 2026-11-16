use serde::Serialize;

/// Where an item came from: the primary transcript or a harness activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemOrigin {
    Primary,
    HarnessActivity,
}

/// Who a text item speaks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextRole {
    User,
    Assistant,
    System,
}
