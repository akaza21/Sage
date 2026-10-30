use super::{CdpPort, CdpTransport, world::left};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const NODES: usize = 20_000;
const CHUNKS: usize = 64;
const RESPONSE_BYTES: usize = 262_144;

impl<T: CdpTransport> CdpPort<T> {
    pub(super) fn annotation_mask_coverage(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut pending = Vec::new();
        let mut visited = 0;
        for chunk in 0..CHUNKS {
            let (method, params, key) = match (chunk, pending.pop()) {
                (0, _) => ("DOM.getDocument", json!({"depth":4, "pierce":true}), "root"),
                (_, Some(backend)) => (
                    "DOM.describeNode",
                    json!({"backendNodeId":backend, "depth":4, "pierce":true}),
                    "node",
                ),
                _ => return true,
            };
            let Ok(timeout) = left(deadline) else {
                return false;
            };
            let Ok(response) =
                self.transport
                    .observe(method, &params.to_string(), RESPONSE_BYTES, timeout)
            else {
                return false;
            };
            let Ok(mut response) = serde_json::from_str::<Value>(&response) else {
                return false;
            };
            let Some(root) = response.get_mut(key).map(Value::take) else {
                return false;
            };
            if !scan_chunk(root, &mut pending, &mut visited) {
                return false;
            }
        }
        pending.is_empty()
    }
}

fn scan_chunk(root: Value, pending: &mut Vec<u64>, visited: &mut usize) -> bool {
    let mut nodes = vec![root];
    while let Some(mut node) = nodes.pop() {
        *visited += 1;
        if *visited > NODES || !node.is_object() {
            return false;
        }
        if node["shadowRootType"]
            .as_str()
            .is_some_and(|kind| kind != "user-agent")
        {
            return false;
        }
        if let Some(roots) = node["shadowRoots"].as_array()
            && roots
                .iter()
                .any(|root| root["shadowRootType"].as_str() != Some("user-agent"))
        {
            return false;
        }
        if node["nodeName"].as_str() == Some("IFRAME") {
            continue;
        }
        let declared = node["childNodeCount"].as_u64().unwrap_or(0);
        let backend = node["backendNodeId"].as_u64();
        let children = node
            .as_object_mut()
            .and_then(|node| node.remove("children"));
        let children = match children {
            Some(Value::Array(children)) => children,
            None => Vec::new(),
            _ => return false,
        };
        if declared > children.len() as u64 {
            let Some(backend) = backend.filter(|id| *id > 0) else {
                return false;
            };
            if pending.len() + *visited >= NODES {
                return false;
            }
            pending.push(backend);
        }
        if nodes.len() + children.len() + *visited > NODES {
            return false;
        }
        nodes.extend(children);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_fail_closed_for_author_shadow_roots_including_standard_hosts() {
        for kind in ["open", "closed"] {
            let node = json!({"nodeName":"DIV", "shadowRoots":[{"shadowRootType":kind}]});
            assert!(!scan_chunk(node, &mut vec![], &mut 0));
        }
        assert!(scan_chunk(
            json!({"nodeName":"INPUT", "shadowRoots":[{"shadowRootType":"user-agent"}]}),
            &mut vec![],
            &mut 0
        ));
    }

    #[test]
    fn coverage_walk_expands_bounded_chunks_and_refuses_over_limit_or_missing_ids() {
        let mut pending = Vec::new();
        let mut visited = 0;
        assert!(scan_chunk(
            json!({"nodeName":"DIV", "backendNodeId":9, "childNodeCount":4}),
            &mut pending,
            &mut visited
        ));
        assert_eq!(pending, [9]);
        assert!(!scan_chunk(
            json!({"childNodeCount":1}),
            &mut vec![],
            &mut 0
        ));
        let mut visited = NODES;
        assert!(!scan_chunk(
            json!({"nodeName":"DIV"}),
            &mut vec![],
            &mut visited
        ));
    }
}
