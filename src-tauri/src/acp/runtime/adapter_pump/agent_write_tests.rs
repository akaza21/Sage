use sha2::{Digest, Sha256};

use super::*;
use crate::acp::resource_mapper::{ConfiguredResourceRoot, NativeResourceMapper};
use sage_acp_protocol::contract::SageRootId;

/// Real files under a real root: the agent's small file, and one past the
/// per-file hashing bound.
fn written_files() -> (std::path::PathBuf, SafeResourceRef, SafeResourceRef) {
    let root = std::env::temp_dir().join(format!("sage-agent-write-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("a.rs"), "fn agent() {}\n").unwrap();
    std::fs::write(root.join("big.bin"), vec![0_u8; 16 * 1024 * 1024 + 1]).unwrap();
    let mapper = NativeResourceMapper::new([ConfiguredResourceRoot {
        root_id: SageRootId::parse("root-one").unwrap(),
        canonical_native_root: root.clone(),
    }])
    .unwrap();
    let map = |name: &str| {
        mapper
            .map_existing(root.join(name).to_str().unwrap())
            .unwrap()
    };
    let (small, big) = (map("a.rs"), map("big.bin"));
    PROBE_ROOTS.with(|roots| *roots.borrow_mut() = Some(mapper));
    (root, small, big)
}

fn recorded(facts: &[NormalizedFact]) -> Vec<(String, Option<[u8; 32]>)> {
    facts
        .iter()
        .map(|fact| match &fact.event {
            DurableAdapterEvent::AgentWrite {
                resource, digest, ..
            } => (resource.relative_path.as_str().to_owned(), *digest),
            _ => panic!("only write records"),
        })
        .collect()
}

#[test]
fn writes_record_only_with_file_preview_once_per_file_and_unread_past_the_bound() {
    let (root, small, big) = written_files();
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(Vec::new());
        let batch = vec![small.clone(), big.clone(), small.clone()];
        assert!(pump.agent_writes(batch.clone(), 16, 1).await.is_empty());

        shared.0.lock().unwrap().file_preview = true;
        let agent: [u8; 32] = Sha256::digest(b"fn agent() {}\n").into();
        assert_eq!(
            recorded(&pump.agent_writes(batch.clone(), 16, 1).await),
            [
                ("big.bin".to_owned(), None),
                ("a.rs".to_owned(), Some(agent))
            ]
        );
        assert_eq!(recorded(&pump.agent_writes(batch, 1, 1).await).len(), 1);
    });
    PROBE_ROOTS.with(|roots| roots.borrow_mut().take());
    let _ = std::fs::remove_dir_all(root);
}
