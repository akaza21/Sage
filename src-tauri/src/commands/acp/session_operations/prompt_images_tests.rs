use super::*;

struct Store {
    directory: std::path::PathBuf,
    database: crate::persistence::Database,
}

impl Store {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("sage-prompt-images-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let database = crate::persistence::Database::open(&directory.join("sage.db")).unwrap();
        {
            let connection = database.connection();
            connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
            for (ordinal, chat) in ["c", "other"].into_iter().enumerate() {
                connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES(?1,'p',?2,'C',1,1)", rusqlite::params![chat, ordinal as i64]).unwrap();
            }
        }
        Self {
            directory,
            database,
        }
    }

    fn image(&self, chat: &str) -> ImageAttachedContext {
        crate::image_artifacts::insert_test_image(
            &self.database.connection(),
            self.database.image_artifact_root().unwrap(),
            "p",
            chat,
            "screenshot.png",
        )
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn an_attached_image_is_read_from_its_owner_bound_artifact_with_a_full_preview() {
    let store = Store::new();
    let image = store.image("c");

    let read = read(&store.database, "p", "c", &[&image]).unwrap();

    assert_eq!(read.len(), 1);
    assert_eq!(read[0].media_type, crate::acp::intent::ImageMediaType::Png);
    assert_eq!(read[0].bytes.len(), image.byte_length as usize);
    assert!(read[0].bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(read[0].preview, ImagePreview::Full);
}

#[test]
fn an_image_from_another_chat_is_refused_with_a_visible_reason() {
    let store = Store::new();
    let foreign = store.image("other");

    let Err(AcpIpcError::InvalidRequest { message }) = read(&store.database, "p", "c", &[&foreign])
    else {
        panic!("a foreign image must be refused");
    };
    assert!(message.as_str().contains("no longer available"));
}

#[test]
fn a_send_target_without_a_session_refuses_images_as_unsupported() {
    let store = Store::new();
    let image = store.image("c");
    let contexts =
        AttachedContexts::new("attachedContext", vec![AttachedContext::Image(image)]).unwrap();

    assert!(matches!(
        image_input_block(&contexts),
        Err(AcpIpcError::Unavailable {
            reason: AcpUnavailableReason::ImageInputUnsupported,
            ..
        })
    ));
    assert!(admit_elements(&contexts).is_ok());
}
