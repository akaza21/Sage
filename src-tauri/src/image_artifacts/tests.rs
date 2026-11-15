use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::persistence::Database;
use sage_acp_protocol::intent::ImageMediaType;

use super::{
    commands::{UploadChatImageRequest, upload_image_payload},
    store::{ingest_image, remove_pending_image},
    verify_image_reference,
};

const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";
const JPEG_1X1: &str = "/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAMCAgMCAgMDAwMEAwMEBQgFBQQEBQoHBwYIDAoMDAsKCwsNDhIQDQ4RDgsLEBYQERMUFRUVDA8XGBYUGBIUFRT/2wBDAQMEBAUEBQkFBQkUDQsNFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBT/wAARCAABAAEDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD50ooor8MP9Uz/2Q==";

#[test]
fn admitted_png_is_retrievable_only_by_its_chat_owner() {
    let directory =
        std::env::temp_dir().join(format!("sage-image-ingest-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
    let bytes = STANDARD.decode(PNG_1X1).unwrap();
    let root = database.image_artifact_root().unwrap();

    let image = ingest_image(
        &connection,
        root,
        "p",
        "c",
        ImageMediaType::Png,
        &bytes,
        "pixel.png",
    )
    .unwrap();

    assert_eq!(image.width, 1);
    assert_eq!(image.height, 1);
    assert_eq!(
        verify_image_reference(&connection, root, "p", "c", &image).unwrap(),
        bytes
    );
    assert!(verify_image_reference(&connection, root, "p", "other", &image).is_err());
    assert!(
        ingest_image(
            &connection,
            root,
            "p",
            "c",
            ImageMediaType::Jpeg,
            &bytes,
            "wrong.jpg"
        )
        .is_err()
    );
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM chat_image_artifacts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    drop(connection);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn png_larger_than_thumbnail_cap_keeps_full_image_and_bounded_preview() {
    let directory = std::env::temp_dir().join(format!("sage-image-large-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
    let mut pixels = vec![0; 512 * 512 * 3];
    for (index, byte) in pixels.iter_mut().enumerate() {
        *byte = (index.wrapping_mul(97).wrapping_add(index / 31) % 251) as u8;
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 512, 512);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    assert!(bytes.len() > 262_144);
    let root = database.image_artifact_root().unwrap();

    let image = ingest_image(
        &connection,
        root,
        "p",
        "c",
        ImageMediaType::Png,
        &bytes,
        "large.png",
    )
    .unwrap();

    assert_eq!(
        verify_image_reference(&connection, root, "p", "c", &image).unwrap(),
        bytes
    );
    let thumbnail = std::fs::read(root.join(format!("{}.thumb", image.artifact_id))).unwrap();
    assert!(thumbnail.len() <= 262_144);
    assert!(
        png::Decoder::new(std::io::Cursor::new(thumbnail))
            .read_info()
            .is_ok()
    );
    drop(connection);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn admitted_jpeg_is_retrievable_with_its_declared_dimensions() {
    let directory = std::env::temp_dir().join(format!("sage-image-jpeg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
    let bytes = STANDARD.decode(JPEG_1X1).unwrap();
    let root = database.image_artifact_root().unwrap();

    let image = ingest_image(
        &connection,
        root,
        "p",
        "c",
        ImageMediaType::Jpeg,
        &bytes,
        "pixel.jpg",
    )
    .unwrap();

    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(
        verify_image_reference(&connection, root, "p", "c", &image).unwrap(),
        bytes
    );
    drop(connection);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn upload_payload_rejects_plus_one_bytes_before_creating_an_artifact() {
    let directory =
        std::env::temp_dir().join(format!("sage-image-upload-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    let request = UploadChatImageRequest {
        project_id: "p".into(),
        project_generation: 1,
        chat_id: "c".into(),
        media_type: ImageMediaType::Png,
        data_base64: STANDARD.encode(vec![0; 4 * 1024 * 1024 + 1]),
        label: "too-large.png".into(),
    };

    assert!(upload_image_payload(&database, request).is_err());
    let count: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM chat_image_artifacts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn pending_image_can_be_removed_only_by_its_chat_owner() {
    let directory =
        std::env::temp_dir().join(format!("sage-image-remove-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
    let root = database.image_artifact_root().unwrap();
    let bytes = STANDARD.decode(PNG_1X1).unwrap();
    let image = ingest_image(
        &connection,
        root,
        "p",
        "c",
        ImageMediaType::Png,
        &bytes,
        "pixel.png",
    )
    .unwrap();

    assert!(remove_pending_image(&connection, root, "p", "foreign", &image.artifact_id).is_err());
    assert_eq!(
        verify_image_reference(&connection, root, "p", "c", &image).unwrap(),
        bytes
    );
    remove_pending_image(&connection, root, "p", "c", &image.artifact_id).unwrap();
    assert!(verify_image_reference(&connection, root, "p", "c", &image).is_err());
    assert!(!root.join(format!("{}.full", image.artifact_id)).exists());
    assert!(!root.join(format!("{}.thumb", image.artifact_id)).exists());
    drop(connection);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn exact_four_mib_png_is_kept_as_uploaded_and_larger_ones_are_normalised() {
    let directory = std::env::temp_dir().join(format!("sage-image-limit-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
    let original = STANDARD.decode(PNG_1X1).unwrap();
    let filler = vec![0; 4 * 1024 * 1024 - original.len() - 12];
    let mut bytes = original[..33].to_vec();
    bytes.extend_from_slice(&(filler.len() as u32).to_be_bytes());
    bytes.extend_from_slice(b"raNd");
    bytes.extend_from_slice(&filler);
    let mut crc = !0_u32;
    for byte in b"raNd".iter().chain(filler.iter()) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320_u32 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    bytes.extend_from_slice(&(!crc).to_be_bytes());
    bytes.extend_from_slice(&original[33..]);
    assert_eq!(bytes.len(), 4 * 1024 * 1024);
    let root = database.image_artifact_root().unwrap();

    let image = ingest_image(
        &connection,
        root,
        "p",
        "c",
        ImageMediaType::Png,
        &bytes,
        "limit.png",
    )
    .unwrap();
    assert_eq!(
        verify_image_reference(&connection, root, "p", "c", &image).unwrap(),
        bytes
    );
    bytes.push(0);
    let normalized = ingest_image(
        &connection,
        root,
        "p",
        "c",
        ImageMediaType::Png,
        &bytes,
        "large.png",
    )
    .unwrap();
    assert_eq!((normalized.width, normalized.height), (1, 1));
    assert!(normalized.byte_length < 1024);
    bytes.resize(sage_core::limits::IMAGE_UPLOAD_BYTES + 1, 0);
    assert!(
        ingest_image(
            &connection,
            root,
            "p",
            "c",
            ImageMediaType::Png,
            &bytes,
            "too-large.png"
        )
        .is_err()
    );
    drop(connection);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_4k_screenshot_is_normalised_at_upload_and_retired_once_sent() {
    let directory = std::env::temp_dir().join(format!("sage-image-sent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let database = Database::open(&directory.join("sage.db")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)", []).unwrap();
    let root = database.image_artifact_root().unwrap();

    // Past the 2048 px store bound: once refused, now stored at that bound for
    // every harness. An image prompt narrows it to 1568 px when it is sent.
    let screenshot = super::screenshot_png(3840, 2160);
    let image = super::ingest_test_image(&connection, root, "p", "c", &screenshot).unwrap();
    assert_eq!((image.width, image.height), (2048, 1152));
    assert_eq!(image.media_type, ImageMediaType::Png);
    let stored = verify_image_reference(&connection, root, "p", "c", &image).unwrap();
    assert_eq!(super::png_dimensions(&stored), (2048, 1152));

    // Even noise is stored for any harness, as the smaller JPEG within 4 MiB;
    // only an image prompt applies the tighter 1 MiB bound.
    let noise = super::noise_png(2100, 1600);
    let noise = super::ingest_test_image(&connection, root, "p", "c", &noise).unwrap();
    assert_eq!(
        (noise.width, noise.height, noise.media_type),
        (2048, 1561, ImageMediaType::Jpeg)
    );
    assert!(noise.byte_length as usize <= sage_core::limits::STORED_IMAGE_BYTES);

    super::retire_sent_images(&connection, root, "p", "c", &[&image.artifact_id]).unwrap();
    assert!(verify_image_reference(&connection, root, "p", "c", &image).is_err());
    assert!(!root.join(format!("{}.full", image.artifact_id)).exists());
    drop(connection);
    drop(database);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn uploads_queue_for_one_image_work_permit_without_holding_the_project_lock() {
    use std::{sync::Arc, time::Duration};
    assert_eq!(sage_core::limits::IMAGE_WORK_PERMITS, 1);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let root =
                std::env::temp_dir().join(format!("sage-image-work-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let database = Database::open(&root.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let chat_id = database
                .bootstrap_snapshot()
                .unwrap()
                .active_tab_id
                .unwrap();
            let state = Arc::new(crate::state::AppState::new(database));
            let generation = *state.project_transitions.lock().await;
            let upload = |state: Arc<crate::state::AppState>| {
                let request = UploadChatImageRequest {
                    project_id: project.id.clone(),
                    project_generation: generation,
                    chat_id: chat_id.clone(),
                    media_type: ImageMediaType::Png,
                    data_base64: STANDARD.encode(super::screenshot_png(3000, 1000)),
                    label: "wide.png".into(),
                };
                tokio::spawn(async move {
                    super::commands::upload_chat_image_inner(&state, request).await
                })
            };

            // A decode in flight holds the only permit; both uploads wait.
            let held = super::image_work_permit().await;
            let first = upload(state.clone());
            let second = upload(state.clone());
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert!(!first.is_finished() && !second.is_finished());
            assert_eq!(super::work::image_work_available(), 0);
            // A project switch is not blocked behind the waiting decodes.
            let switch =
                tokio::time::timeout(Duration::from_millis(200), state.project_transitions.lock())
                    .await;
            assert!(switch.is_ok());
            drop(switch);

            drop(held);
            let first = first.await.unwrap().unwrap();
            let second = second.await.unwrap().unwrap();
            assert_ne!(first.artifact_id, second.artifact_id);
            assert_eq!((first.width, first.height), (2048, 683));
            assert_eq!(super::work::image_work_available(), 1);
            drop(state);
            let _ = std::fs::remove_dir_all(root);
        });
}
