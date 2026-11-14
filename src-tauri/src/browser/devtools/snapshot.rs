use crate::acp::browser::{BrowserFailure, redact_console};
use sage_core::limits::BROWSER_LIMITS;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Default))]
pub(crate) struct DocumentStamp {
    pub navigation_id: u64,
    pub generation: u64,
}

pub(crate) struct NativeNode {
    pub backend_id: u64,
    pub depth: usize,
    pub role: String,
    pub name: String,
}

pub(super) fn project(nodes: Vec<NativeNode>) -> Result<(String, Vec<NativeNode>), BrowserFailure> {
    let mut selected = Vec::new();
    let mut text = String::new();
    for mut node in nodes {
        if node.depth > BROWSER_LIMITS.snapshot_depth || node.backend_id == 0 {
            continue;
        }
        if selected.len() == BROWSER_LIMITS.snapshot_nodes {
            break;
        }
        node.role = bounded(&redact_console(&node.role), BROWSER_LIMITS.node_role_bytes);
        node.name = bounded(&redact_console(&node.name), BROWSER_LIMITS.node_name_bytes);
        if !node.name.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            let remaining = BROWSER_LIMITS
                .snapshot_text_bytes
                .saturating_sub(text.len());
            text.push_str(&bounded(&node.name, remaining));
        }
        selected.push(node);
    }
    Ok((text, selected))
}

pub(super) fn parse_ax_tree(payload: &str) -> Result<Vec<NativeNode>, BrowserFailure> {
    if payload.len() > 1_048_576 {
        return Err(BrowserFailure::Capacity);
    }
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|_| BrowserFailure::Unavailable)?;
    let nodes = value["nodes"]
        .as_array()
        .ok_or(BrowserFailure::Unavailable)?;
    if nodes.len() > BROWSER_LIMITS.snapshot_nodes * 16 {
        return Err(BrowserFailure::Capacity);
    }
    let by_id: HashMap<&str, &serde_json::Value> = nodes
        .iter()
        .filter_map(|node| node["nodeId"].as_str().map(|id| (id, node)))
        .collect();
    let root = nodes
        .first()
        .and_then(|node| node["nodeId"].as_str())
        .ok_or(BrowserFailure::Unavailable)?;
    let mut queue = VecDeque::from([(root, 0usize)]);
    let mut selected = Vec::new();
    let mut visited = std::collections::HashSet::new();
    while let Some((id, depth)) = queue.pop_front() {
        if !visited.insert(id) || depth > BROWSER_LIMITS.snapshot_depth {
            continue;
        }
        let Some(node) = by_id.get(id) else {
            continue;
        };
        if let Some(children) = node["childIds"].as_array() {
            for child in children.iter().filter_map(|value| value.as_str()) {
                queue.push_back((child, depth + 1));
            }
        }
        let Some(backend_id) = node["backendDOMNodeId"].as_u64() else {
            continue;
        };
        if node["ignored"].as_bool().unwrap_or(false) {
            continue;
        }
        let Some(role) = node["role"]["value"].as_str() else {
            continue;
        };
        let name = node["name"]["value"].as_str().unwrap_or("");
        selected.push(NativeNode {
            backend_id,
            depth,
            role: role.into(),
            name: name.into(),
        });
        if selected.len() >= BROWSER_LIMITS.snapshot_nodes {
            break;
        }
    }
    Ok(selected)
}

fn bounded(value: &str, cap: usize) -> String {
    let mut end = value.len().min(cap);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod ax_tests {
    use super::*;

    #[test]
    fn full_ax_tree_projects_named_nodes_in_document_order() {
        let payload = r#"{"nodes":[
            {"nodeId":"root","role":{"value":"RootWebArea"},"childIds":["a","b"]},
            {"nodeId":"a","backendDOMNodeId":11,"role":{"value":"button"},"name":{"value":"Continue"}},
            {"nodeId":"b","backendDOMNodeId":12,"role":{"value":"textbox"},"name":{"value":"Email"}}
        ]}"#;
        let nodes = parse_ax_tree(payload).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(
            (
                nodes[0].backend_id,
                nodes[0].depth,
                nodes[0].role.as_str(),
                nodes[0].name.as_str()
            ),
            (11, 1, "button", "Continue")
        );
        assert_eq!((nodes[1].backend_id, nodes[1].depth), (12, 1));
    }

    #[test]
    fn malformed_or_oversized_ax_tree_fails_closed() {
        assert_eq!(parse_ax_tree("{}").err(), Some(BrowserFailure::Unavailable));
        let payload = format!(
            "{{\"nodes\":[{}]}}",
            vec!["{}"; BROWSER_LIMITS.snapshot_nodes * 32].join(",")
        );
        assert_eq!(
            parse_ax_tree(&payload).err(),
            Some(BrowserFailure::Capacity)
        );
    }
}
