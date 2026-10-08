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
        assert!(
            crate::image_artifacts::load_image_preview(
                &database.connection(),
                root,
                "p",
                "c",
                &pending.artifact_id
            )
            .unwrap()
            .is_none()
        );
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
        assert!(
            crate::image_artifacts::load_image_preview(
                &database.connection(),
                root,
                "p",
                "c",
                &queued.artifact_id
            )
            .unwrap()
            .is_some()
        );
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
