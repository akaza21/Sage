use super::*;

#[test]
fn reopen_discards_pending_image_but_preserves_queued_image() {
    let directory =
        std::env::temp_dir().join(format!("sage-image-reopen-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("sage.db");
    let (pending, queued);
    {
        let database = Database::open(&path).unwrap();
        database.connection().execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
        database.connection().execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
        pending = crate::image_artifacts::insert_test_image(
            &database.connection(),
            database.image_artifact_root().unwrap(),
            "p",
            "c",
            "pending.png",
        );
        queued = crate::image_artifacts::insert_test_image(
            &database.connection(),
            database.image_artifact_root().unwrap(),
            "p",
            "c",
            "queued.png",
        );
        database
            .replace_chat_queued_prompts(
                "p",
                "c",
                &[DurableQueuedPrompt {
                    id: "queue-1".into(),
                    text: "later".into(),
                    attached_context: AttachedContexts::new(
                        "attachedContext",
                        vec![AttachedContext::Image(queued.clone())],
                    )
                    .unwrap(),
                    binding_id: "binding".into(),
                    model_id: None,
                    status: QueuedPromptStatus::Queued,
                }],
            )
            .unwrap();
    }
    {
        let database = Database::open(&path).unwrap();
        let root = database.image_artifact_root().unwrap();
        let pending_rows: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM chat_image_artifacts WHERE id=?1",
                [&pending.artifact_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending_rows, 0);
        assert!(!root.join(format!("{}.full", pending.artifact_id)).exists());
        assert!(!root.join(format!("{}.thumb", pending.artifact_id)).exists());
        assert_eq!(
            database.project_chat_queued_prompts("p").unwrap()["c"].len(),
            1
        );
        assert!(
            crate::image_artifacts::verify_image_reference(
                &database.connection(),
                root,
                "p",
                "c",
                &queued
            )
            .is_ok()
        );
        assert!(root.join(format!("{}.thumb", queued.artifact_id)).exists());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn queued_image_identity_is_committed_with_the_prompt_and_retired_on_removal() {
    let directory = std::env::temp_dir().join(format!("sage-image-queue-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    database.connection().execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
    database.connection().execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
    let image = crate::image_artifacts::insert_test_image(
        &database.connection(),
        database.image_artifact_root().unwrap(),
        "p",
        "c",
        "image.png",
    );
    let prompt = DurableQueuedPrompt {
        id: "queue-1".into(),
        text: "later".into(),
        attached_context: AttachedContexts::new(
            "attachedContext",
            vec![AttachedContext::Image(image.clone())],
        )
        .unwrap(),
        binding_id: "binding".into(),
        model_id: None,
        status: QueuedPromptStatus::Queued,
    };
    database
        .replace_chat_queued_prompts("p", "c", &[prompt])
        .unwrap();
    let queued: (String, String) = database
        .connection()
        .query_row(
            "SELECT lifecycle,queue_id FROM chat_image_artifacts WHERE id=?1",
            [&image.artifact_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(queued, ("queued".into(), "queue-1".into()));
    assert_eq!(
        database.project_chat_queued_prompts("p").unwrap()["c"].len(),
        1
    );
    database
        .connection()
        .execute(
            "UPDATE chat_image_artifacts SET queue_id='wrong' WHERE id=?1",
            [&image.artifact_id],
        )
        .unwrap();
    assert!(database.project_chat_queued_prompts("p").is_err());
    database
        .connection()
        .execute(
            "UPDATE chat_image_artifacts SET queue_id='queue-1' WHERE id=?1",
            [&image.artifact_id],
        )
        .unwrap();
    database.replace_chat_queued_prompts("p", "c", &[]).unwrap();
    let state: String = database
        .connection()
        .query_row(
            "SELECT lifecycle FROM chat_image_artifacts WHERE id=?1",
            [&image.artifact_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "retired");
    assert!(
        !database
            .image_artifact_root()
            .unwrap()
            .join(format!("{}.full", image.artifact_id))
            .exists()
    );
    assert!(
        database
            .image_artifact_root()
            .unwrap()
            .join(format!("{}.thumb", image.artifact_id))
            .exists()
    );
    drop(database);
    assert!(directory.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn queue_survives_database_reopen_but_requires_explicit_retry() {
    let directory = std::env::temp_dir().join(format!("sage-chat-queue-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("sage.db");
    {
        let database = Database::open(&path).unwrap();
        let connection = database.connection();
        connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES ('p','P',1,1,1)", []).unwrap();
        connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES ('c','p',0,'C',1,1)", []).unwrap();
        drop(connection);
        database
            .replace_chat_queued_prompts(
                "p",
                "c",
                &[DurableQueuedPrompt {
                    id: "q1".into(),
                    text: "later".into(),
                    attached_context: Default::default(),
                    binding_id: "binding".into(),
                    model_id: None,
                    status: QueuedPromptStatus::Queued,
                }],
            )
            .unwrap();
    }
    {
        let database = Database::open(&path).unwrap();
        let restored = database.project_chat_queued_prompts("p").unwrap();
        let queue = &restored["c"];
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].text, "later");
        assert!(matches!(queue[0].status, QueuedPromptStatus::Blocked));
        database.replace_chat_queued_prompts("p", "c", &[]).unwrap();
        assert!(
            database
                .project_chat_queued_prompts("p")
                .unwrap()
                .is_empty()
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn outcome_unknown_queue_status_survives_database_reopen() {
    let directory = std::env::temp_dir().join(format!("sage-chat-queue-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("sage.db");
    {
        let database = Database::open(&path).unwrap();
        let connection = database.connection();
        connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES ('p','P',1,1,1)", []).unwrap();
        connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES ('c','p',0,'C',1,1)", []).unwrap();
        drop(connection);
        database
            .replace_chat_queued_prompts(
                "p",
                "c",
                &[DurableQueuedPrompt {
                    id: "same-client-id".into(),
                    text: "maybe delivered".into(),
                    attached_context: Default::default(),
                    binding_id: "binding".into(),
                    model_id: None,
                    status: QueuedPromptStatus::Queued,
                }],
            )
            .unwrap();
        database.connection().execute("UPDATE chat_queued_prompts SET prompts_json=json_set(prompts_json,'$[0].status','outcomeUnknown') WHERE chat_id='c'", []).unwrap();
    }
    let database = Database::open(&path).unwrap();
    let restored = database.project_chat_queued_prompts("p");
    assert!(restored.is_ok());
    let queue = &restored.unwrap()["c"];
    assert_eq!(queue[0].id, "same-client-id");
    assert_eq!(
        serde_json::to_value(queue[0].status).unwrap(),
        serde_json::json!("outcomeUnknown")
    );
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn an_attachment_only_prompt_queues_with_empty_text_and_a_blank_one_does_not() {
    let prompt = |text: &str, context: serde_json::Value| -> DurableQueuedPrompt {
        serde_json::from_value(serde_json::json!({
            "id": "queue-1",
            "text": text,
            "attachedContext": context,
            "bindingId": "binding",
            "modelId": null,
            "status": "queued"
        }))
        .unwrap()
    };
    let quote = serde_json::json!([{
        "kind": "transcriptQuote", "ordinal": 1, "sourceTurnId": "t",
        "sourceItemId": "i", "text": "quote", "startOffset": 0, "endOffset": 5
    }]);
    assert!(admitted_prompts(&[prompt("  ", quote)]).is_ok());
    assert!(admitted_prompts(&[prompt("  ", serde_json::json!([]))]).is_err());
}

fn element_prompt(id: &str, elements: Vec<(u8, &str)>) -> DurableQueuedPrompt {
    let context: Vec<_> = elements
        .into_iter()
        .map(|(ordinal, name)| {
            serde_json::json!({
                "kind": "browserElement", "target": "element", "ordinal": ordinal,
                "tabId": "browser-1", "url": "https://example.com/", "title": "Home",
                "selector": "main > button", "role": "button", "name": name,
                "text": "", "box": {"x": 0, "y": 0, "width": 10, "height": 10},
                "comment": ""
            })
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": id, "text": "later", "attachedContext": context,
        "bindingId": "binding", "modelId": null, "status": "queued"
    }))
    .unwrap()
}

/// A queued page element stored before a stricter redaction rule must not
/// stop its Project from opening, nor block adding or removing the other
/// queued prompts; only a new or edited prompt is admitted afresh.
#[test]
fn a_stored_element_that_fails_a_stricter_rule_still_loads_and_only_new_writes_are_refused() {
    let directory =
        std::env::temp_dir().join(format!("sage-queue-element-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
    let stale = element_prompt("p1", vec![(1, "ghp_0123456789abcdefABCDEF")]);
    let second = element_prompt("p2", vec![(1, "Buy")]);
    let third = element_prompt("p3", vec![(1, "Sell")]);
    connection
        .execute(
            "INSERT INTO chat_queued_prompts(project_id,chat_id,prompts_json,updated_at) VALUES('p','c',?1,1)",
            [serde_json::to_string(&vec![stale.clone(), second.clone()]).unwrap()],
        )
        .unwrap();
    drop(connection);
    assert_eq!(
        database.project_chat_queued_prompts("p").unwrap()["c"].len(),
        2
    );
    // Adding P3, then deleting P2, passes the unchanged stale P1 through.
    database
        .replace_chat_queued_prompts("p", "c", &[stale.clone(), second, third.clone()])
        .unwrap();
    database
        .replace_chat_queued_prompts("p", "c", &[stale.clone(), third.clone()])
        .unwrap();
    // Editing P1 admits it afresh, and it no longer passes.
    let mut edited = stale;
    edited.text = "sooner".into();
    let refused = database
        .replace_chat_queued_prompts("p", "c", &[edited, third.clone()])
        .unwrap_err();
    assert!(
        refused.to_string().contains("no longer passes redaction"),
        "{refused}"
    );
    let repeated = element_prompt("p4", vec![(1, "Buy"), (1, "Sell")]);
    let refused = database
        .replace_chat_queued_prompts("p", "c", &[third, repeated])
        .unwrap_err();
    assert!(refused.to_string().contains("share a number"), "{refused}");
    assert_eq!(
        database.project_chat_queued_prompts("p").unwrap()["c"].len(),
        2
    );
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}
