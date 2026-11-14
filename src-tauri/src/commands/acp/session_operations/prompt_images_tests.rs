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

#[test]
fn a_4k_screenshot_is_accepted_and_sent_at_most_1568_px_wide() {
    let store = Store::new();
    let upload = |bytes: Vec<u8>| {
        crate::image_artifacts::ingest_test_image(
            &store.database.connection(),
            store.database.image_artifact_root().unwrap(),
            "p",
            "c",
            &bytes,
        )
        .unwrap()
    };
    // Normalised at upload; a 2000 px image within the store bounds is kept
    // as uploaded and normalised when the prompt is sent.
    let uhd = upload(crate::image_artifacts::screenshot_png(3840, 2160));
    let wide = upload(crate::image_artifacts::screenshot_png(2000, 1000));
    assert_eq!(wide.width, 2000);

    let read = read(&store.database, "p", "c", &[&uhd, &wide]).unwrap();

    for (image, expected) in read.iter().zip([(1568, 882), (1568, 784)]) {
        assert_eq!(image.media_type, crate::acp::intent::ImageMediaType::Png);
        assert_eq!(
            crate::image_artifacts::png_dimensions(&image.bytes),
            expected
        );
        assert!(image.preview().is_some());
    }
    assert!(
        admit_sizes(
            read.iter().map(|image| image.bytes.len()),
            PROMPT_IMAGE_TOTAL_BYTES
        )
        .is_ok()
    );
}

#[test]
fn an_image_still_too_large_after_resizing_is_refused_with_the_prompt_bounds() {
    let store = Store::new();
    let noise = crate::image_artifacts::ingest_test_image(
        &store.database.connection(),
        store.database.image_artifact_root().unwrap(),
        "p",
        "c",
        &crate::image_artifacts::noise_png(2100, 1600),
    )
    .unwrap();

    let Err(AcpIpcError::InvalidRequest { message }) = read(&store.database, "p", "c", &[&noise])
    else {
        panic!("noise past the prompt bound must be refused");
    };
    assert!(
        message
            .as_str()
            .contains("at most 1 MiB after resizing to 1568 px"),
        "{}",
        message.as_str()
    );
}

#[test]
fn codex_and_pi_take_the_echoed_image_bound_and_others_the_full_one() {
    use crate::acp::limits::ECHOED_PROMPT_IMAGE_TOTAL_BYTES;
    for binding in ["codex-app-server", "pi-rpc"] {
        let (bounds, total) = super::prompt_bounds(binding);
        assert_eq!(
            (bounds.bytes, total),
            (
                ECHOED_PROMPT_IMAGE_TOTAL_BYTES,
                ECHOED_PROMPT_IMAGE_TOTAL_BYTES
            )
        );
    }
    for binding in ["claude-code-sdk", "claude-agent-acp", "opencode-acp"] {
        let (bounds, total) = super::prompt_bounds(binding);
        assert_eq!(
            (bounds.bytes, total),
            (PROMPT_IMAGE_TOTAL_BYTES, PROMPT_IMAGE_TOTAL_BYTES)
        );
    }
}
