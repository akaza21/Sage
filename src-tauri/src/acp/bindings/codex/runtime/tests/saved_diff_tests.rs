use super::*;

struct SavedResource;

impl ResourceMapper for SavedResource {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        Ok(SafeResourceRef::new(SageRootId::parse("root-runtime").unwrap(), "src/lib.rs").unwrap())
    }
}

#[test]
fn completed_codex_patch_delivers_metadata_and_pair_to_the_shared_journal() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    let path = std::env::current_dir().unwrap().join("src/lib.rs");
    let batch = runtime
        .ingest(
            &frame(json!({
                "method":"item/completed", "params":{
                    "threadId":"thread-1", "turnId":"turn-1", "item":{
                        "type":"fileChange", "id":"saved-edit", "status":"completed",
                        "changes":[{"path":path, "kind":{"type":"update"},
                            "diff":"@@ -1 +1,2 @@\n-old\n+new\n+extra\n"}]
                    }
                }
            })),
            &SavedResource,
        )
        .unwrap();
    let [
        AdapterEvent::Item {
            id: edit_id,
            payload: edit,
            ..
        },
        AdapterEvent::Item { id, payload, .. },
        AdapterEvent::FilePair {
            id: pair_id,
            external_item_id,
            old_text,
            new_text,
        },
    ] = batch.events()
    else {
        panic!("a saved edit must deliver its metadata and text in one batch");
    };
    assert_eq!(id, pair_id);
    assert_eq!(external_item_id.as_str(), "saved-edit");
    assert_eq!(edit_id.external_id.as_str(), "saved-edit:edit");
    assert!(matches!(
        edit,
        NormalizedPayload::ToolCall {
            category: crate::acp::intent::ToolCategory::Edit,
            label,
            ..
        } if label.as_str() == "Edited src/lib.rs"
    ));
    assert!(matches!(
        payload,
        NormalizedPayload::FileChangeDiff {
            pair_available: true,
            additions: Some(2),
            deletions: Some(1),
            ..
        }
    ));
    assert_eq!(old_text, "old\n");
    assert_eq!(new_text, "new\nextra\n");
}
