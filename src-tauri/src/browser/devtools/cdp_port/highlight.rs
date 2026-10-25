use serde_json::json;
use std::time::Duration;

use super::{CdpPort, CdpTransport};
use crate::browser::devtools::snapshot::DocumentStamp;

pub(super) fn mark<T: CdpTransport>(port: &CdpPort<T>, stamp: DocumentStamp, backend_id: u64) {
    if port.stamp().ok() != Some(stamp) {
        return;
    }
    if port
        .call("Overlay.enable", "{}", 4096, Duration::from_secs(1))
        .is_err()
    {
        return;
    }
    let _ = port.call(
        "Overlay.highlightNode",
        &highlight_params(backend_id),
        4096,
        Duration::from_secs(1),
    );
}

fn highlight_params(backend_id: u64) -> String {
    json!({
        "backendNodeId": backend_id,
        "highlightConfig": {
            "showInfo": false,
            "showStyles": false,
            "showRulers": false,
            "contentColor": {"r": 74, "g": 143, "b": 255, "a": 0.24},
            "borderColor": {"r": 74, "g": 143, "b": 255, "a": 0.9}
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_uses_only_a_native_document_node_id() {
        let params: serde_json::Value = serde_json::from_str(&highlight_params(42)).unwrap();
        assert_eq!(params["backendNodeId"], 42);
        assert_eq!(params["highlightConfig"]["showInfo"], false);
        assert!(!highlight_params(42).contains("value"));
    }
}
