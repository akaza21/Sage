use std::{cell::Cell, collections::BTreeSet};
use tokio::io::AsyncWriteExt;

use crate::acp::limits::LIMITS;

use super::{
    BoundedQueue, FrameError, FrameReader, QueueError, ReservedQueue, StructuralError,
    request_payload_digest, validate_structure,
};

async fn frame_from(parts: &[&[u8]]) -> Result<super::Frame, FrameError> {
    let (mut writer, reader) = tokio::io::duplex(2 * 1024 * 1024);
    for part in parts {
        writer.write_all(part).await.expect("write fixture bytes");
    }
    drop(writer);
    FrameReader::new(reader)
        .read_frame()
        .await?
        .ok_or(FrameError::PartialEof)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestDigestVectors {
    method_utf8: String,
    vectors: Vec<RequestDigestVector>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestDigestVector {
    id: String,
    params_present: bool,
    params_token_utf8: String,
    sha256: String,
}

#[test]
fn exact_request_payload_digest_matches_every_frozen_vector() {
    let vectors: RequestDigestVectors = serde_json::from_str(include_str!(
        "../../../../docs/work-items/ADE-0006A-acp-session-foundation/contracts/request-digest-vectors-v1.json"
    ))
    .unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut seen = BTreeSet::new();
            for vector in vectors.vectors {
                let json = if vector.params_present {
                    format!(
                        "{{\"jsonrpc\":\"2.0\",\"method\":\"{}\",\"params\":{}}}",
                        vectors.method_utf8, vector.params_token_utf8
                    )
                } else {
                    format!(
                        "{{\"jsonrpc\":\"2.0\",\"method\":\"{}\"}}",
                        vectors.method_utf8
                    )
                };
                let line = format!("{json}\n");
                let frame = frame_from(&[line.as_bytes()]).await.unwrap();
                validate_structure(&frame).unwrap();
                let actual = request_payload_digest(&frame)
                    .unwrap()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                assert_eq!(actual, vector.sha256, "{}", vector.id);
                assert!(seen.insert(actual), "digest alias for {}", vector.id);
            }
            assert_eq!(seen.len(), 12);

            let duplicate = frame_from(&[
                br#"{"method":"session/prompt","params":{"a":1,"\u0061":2}}"#,
                b"\n",
            ])
            .await
            .unwrap();
            assert_eq!(
                validate_structure(&duplicate),
                Err(StructuralError::DuplicateKey)
            );
        });
}

#[test]
fn fragmented_crlf_and_multiple_frames_are_preserved() {
    runtime().block_on(async {
        let (mut writer, reader) = tokio::io::duplex(256);
        writer.write_all(b"{\"json").await.unwrap();
        writer
            .write_all(b"rpc\":\"2.0\",\"id\":1}\r\n{\"jsonrpc\":\"2.0\",\"id\":2}\n")
            .await
            .unwrap();
        drop(writer);
        let mut reader = FrameReader::new(reader);
        let first = reader.read_frame().await.unwrap().unwrap();
        let second = reader.read_frame().await.unwrap().unwrap();
        assert_eq!(first.as_bytes(), br#"{"jsonrpc":"2.0","id":1}"#);
        assert_eq!(second.as_bytes(), br#"{"jsonrpc":"2.0","id":2}"#);
        assert!(reader.read_frame().await.unwrap().is_none());
    });
}

#[test]
fn frame_exact_limit_and_plus_one_are_distinct() {
    runtime().block_on(async {
        let limit = usize::try_from(LIMITS.wire.frame_bytes).unwrap();
        let mut exact = vec![b' '; limit];
        exact.extend_from_slice(b"\r\n");
        assert_eq!(frame_from(&[&exact]).await.unwrap().len(), limit);

        let mut over = vec![b' '; limit + 1];
        over.push(b'\n');
        let (mut writer, input) = tokio::io::duplex(limit + 2);
        writer.write_all(&over).await.unwrap();
        drop(writer);
        let mut reader = FrameReader::new(input);
        assert_eq!(reader.read_frame().await.err(), Some(FrameError::Oversize));
        assert!(reader.assembly_capacity() <= limit);
    });
}

#[test]
fn malformed_delimiters_nul_utf8_and_partial_eof_fail_closed() {
    runtime().block_on(async {
        assert_eq!(
            frame_from(&[b"{}\r "]).await.err(),
            Some(FrameError::UnexpectedDelimiter)
        );
        assert_eq!(frame_from(&[b"{\0}\n"]).await.err(), Some(FrameError::Nul));
        assert_eq!(
            frame_from(&[&[b'{', 0xff, b'}', b'\n']]).await.err(),
            Some(FrameError::InvalidUtf8)
        );
        assert_eq!(
            frame_from(&[b"{}"]).await.err(),
            Some(FrameError::PartialEof)
        );
    });
}

#[test]
fn structural_validator_rejects_duplicate_batch_and_bounded_fields() {
    runtime().block_on(async {
        let duplicate = frame_from(&[b"{\"id\":1,\"id\":2}\n"]).await.unwrap();
        assert_eq!(
            validate_structure(&duplicate),
            Err(StructuralError::DuplicateKey)
        );
        let batch = frame_from(&[b"[]\n"]).await.unwrap();
        assert_eq!(validate_structure(&batch), Err(StructuralError::Batch));

        let method = "m".repeat(usize::try_from(LIMITS.wire.method_bytes + 1).unwrap());
        let encoded = format!("{{\"method\":\"{method}\"}}\n");
        let method_frame = frame_from(&[encoded.as_bytes()]).await.unwrap();
        assert_eq!(
            validate_structure(&method_frame),
            Err(StructuralError::MethodTooLarge)
        );
        let nested_method = "m".repeat(usize::try_from(LIMITS.wire.method_bytes + 1).unwrap());
        let encoded = format!("{{\"params\":{{\"method\":\"{nested_method}\"}}}}\n");
        let nested_method_frame = frame_from(&[encoded.as_bytes()]).await.unwrap();
        assert_eq!(validate_structure(&nested_method_frame), Ok(()));

        let number = "1".repeat(usize::try_from(LIMITS.wire.numeric_token_bytes + 1).unwrap());
        let encoded = format!("{{\"id\":{number}}}\n");
        let number_frame = frame_from(&[encoded.as_bytes()]).await.unwrap();
        assert_eq!(
            validate_structure(&number_frame),
            Err(StructuralError::NumericTokenTooLarge)
        );
    });
}

#[test]
fn frozen_structural_boundaries_accept_exact_and_reject_plus_one() {
    runtime().block_on(async {
        async fn validate(json: String) -> Result<(), StructuralError> {
            let line = format!("{json}\n");
            let frame = frame_from(&[line.as_bytes()]).await.unwrap();
            validate_structure(&frame)
        }

        let nested = |depth: usize| format!("{}0{}", "{\"x\":".repeat(depth), "}".repeat(depth));
        assert_eq!(validate(nested(32)).await, Ok(()));
        assert_eq!(validate(nested(33)).await, Err(StructuralError::Depth));

        let object = |count: usize| {
            let members = (0..count)
                .map(|index| format!("\"k{index}\":0"))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{members}}}")
        };
        assert_eq!(validate(object(256)).await, Ok(()));
        assert_eq!(
            validate(object(257)).await,
            Err(StructuralError::ObjectTooLarge)
        );

        let array = |count: usize| format!("{{\"values\":[{}]}}", vec!["0"; count].join(","));
        assert_eq!(validate(array(512)).await, Ok(()));
        assert_eq!(
            validate(array(513)).await,
            Err(StructuralError::ArrayTooLarge)
        );
        let nodes = |count: usize| {
            let mut scalar_nodes = count.saturating_sub(256);
            let members = (0..256)
                .map(|index| {
                    let remaining_arrays = 256 - index;
                    let entries = scalar_nodes.div_ceil(remaining_arrays);
                    scalar_nodes -= entries;
                    format!("\"k{index}\":[{}]", vec!["0"; entries].join(","))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{members}}}")
        };
        assert_eq!(validate(nodes(16_384)).await, Ok(()));
        assert_eq!(validate(nodes(16_385)).await, Err(StructuralError::Nodes));

        let key = |bytes: usize| format!("{{\"{}\":0}}", "k".repeat(bytes));
        assert_eq!(validate(key(256)).await, Ok(()));
        assert_eq!(
            validate(key(257)).await,
            Err(StructuralError::DecodedKeyTooLarge)
        );

        let method = |bytes: usize| format!("{{\"method\":\"{}\"}}", "m".repeat(bytes));
        assert_eq!(validate(method(128)).await, Ok(()));
        assert_eq!(
            validate(method(129)).await,
            Err(StructuralError::MethodTooLarge)
        );

        let number = |bytes: usize| format!("{{\"id\":{}}}", "1".repeat(bytes));
        assert_eq!(validate(number(64)).await, Ok(()));
        assert_eq!(
            validate(number(65)).await,
            Err(StructuralError::NumericTokenTooLarge)
        );

        assert_eq!(
            validate("{}{}".to_owned()).await,
            Err(StructuralError::Syntax)
        );
    });
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn bounded_queue_reserves_before_push_and_releases_once() {
    let mut queue = BoundedQueue::new(2, 4);
    assert_eq!(queue.allocated_capacity(), 0);
    queue.try_push_with(2, || 1).unwrap();
    queue.try_push_with(2, || 2).unwrap();
    let builds = Cell::new(0);
    assert_eq!(
        queue.try_push_with(0, || {
            builds.set(builds.get() + 1);
            3
        }),
        Err(QueueError::CountCapacity)
    );
    assert_eq!(builds.get(), 0);
    assert_eq!(queue.pop(), Some(1));
    assert_eq!(queue.retained_bytes(), 2);
    assert_eq!(queue.pop(), Some(2));
    assert_eq!(queue.retained_bytes(), 0);
    assert_eq!(queue.pop(), None);

    let mut byte_bounded = BoundedQueue::new(2, 4);
    byte_bounded.try_push_with(4, || 1).unwrap();
    let builds = Cell::new(0);
    assert_eq!(
        byte_bounded.try_push_with(1, || {
            builds.set(builds.get() + 1);
            2
        }),
        Err(QueueError::ByteCapacity)
    );
    assert_eq!(builds.get(), 0);
}

#[test]
fn saturated_data_cannot_consume_control_reserve() {
    let mut queue = ReservedQueue::new(4, 128, 1, 32).unwrap();
    assert_eq!(queue.allocated_capacities(), (0, 0));
    for value in 0..3 {
        queue.try_push_data_with(32, || value).unwrap();
    }
    let data_builds = Cell::new(0);
    assert_eq!(
        queue.try_push_data_with(1, || {
            data_builds.set(data_builds.get() + 1);
            3
        }),
        Err(QueueError::CountCapacity)
    );
    assert_eq!(data_builds.get(), 0);
    queue.try_push_control_with(32, 10, || 99).unwrap();
    let control_builds = Cell::new(0);
    assert_eq!(
        queue.try_push_control_with(0, 0, || {
            control_builds.set(control_builds.get() + 1);
            100
        }),
        Err(QueueError::CountCapacity)
    );
    assert_eq!(control_builds.get(), 0);
    assert_eq!(queue.data_usage(), (3, 96));
    assert_eq!(queue.control_usage(), (1, 32));
    assert_eq!(queue.pop(), Some(99));
    assert_eq!(queue.pop(), Some(0));
}

#[test]
fn control_lane_is_earliest_deadline_then_fifo() {
    let mut queue = ReservedQueue::new(8, 1024, 4, 256).unwrap();
    queue.try_push_data_with(1, || "data").unwrap();
    queue.try_push_control_with(1, 20, || "late").unwrap();
    queue.try_push_control_with(1, 10, || "early-a").unwrap();
    queue.try_push_control_with(1, 10, || "early-b").unwrap();
    assert_eq!(queue.pop(), Some("early-a"));
    assert_eq!(queue.pop(), Some("early-b"));
    assert_eq!(queue.pop(), Some("late"));
    assert_eq!(queue.pop(), Some("data"));
}

#[test]
fn control_only_pop_preserves_data_and_updates_control_accounting() {
    let mut queue = ReservedQueue::new(4, 128, 2, 32).unwrap();
    queue.try_push_data_with(7, || "data").unwrap();
    queue.try_push_control_with(9, 20, || "control").unwrap();

    assert_eq!(queue.data_usage(), (1, 7));
    assert_eq!(queue.control_usage(), (1, 9));
    assert_eq!(queue.pop_control(), Some("control"));
    assert_eq!(queue.control_usage(), (0, 0));
    assert_eq!(queue.data_usage(), (1, 7));
    assert_eq!(queue.pop_control(), None);
    assert_eq!(queue.pop(), Some("data"));
}

#[test]
fn a_full_budget_prompt_queues_beside_a_full_data_lane() {
    use crate::acp::limits::{PROMPT_EGRESS_RESERVE_BYTES, PROMPT_FRAME_BYTES};
    let mut queue = ReservedQueue::new(
        LIMITS.queues.egress_frames as usize,
        LIMITS.queues.egress_bytes as usize,
        LIMITS.queues.egress_control_frames_inside_total as usize,
        LIMITS.queues.egress_control_bytes_inside_total as usize,
    )
    .unwrap()
    .with_prompt_reserve(PROMPT_EGRESS_RESERVE_BYTES);
    // Ordinary frames of the largest size fill the data lane.
    let frame = LIMITS.wire.frame_bytes as usize + 1;
    let mut queued = 0;
    while queue.try_push_data_with(frame, || queued).is_ok() {
        queued += 1;
    }
    assert!(queued >= 1);
    let (_, data_bytes) = queue.data_usage();

    queue
        .try_push_prompt_with(PROMPT_FRAME_BYTES + 1, || usize::MAX)
        .unwrap();
    assert_eq!(queue.data_usage().1, data_bytes + PROMPT_FRAME_BYTES + 1);
    // The reserve holds one prompt; the data lane stays as full as it was.
    assert_eq!(
        queue.try_push_prompt_with(1, || 0),
        Err(QueueError::CountCapacity)
    );
    assert_eq!(
        queue.try_push_data_with(frame, || 0),
        Err(QueueError::ByteCapacity)
    );
    // Data order is kept, and the reserve is released when the prompt leaves.
    for value in 0..queued {
        assert_eq!(queue.pop(), Some(value));
    }
    assert_eq!(queue.pop(), Some(usize::MAX));
    assert_eq!(queue.data_usage(), (0, 0));
    queue
        .try_push_prompt_with(PROMPT_FRAME_BYTES + 1, || 1)
        .unwrap();
    assert_eq!(
        queue.try_push_prompt_with(PROMPT_EGRESS_RESERVE_BYTES + 1, || 2),
        Err(QueueError::CountCapacity)
    );
}

#[test]
fn a_queue_without_a_prompt_reserve_refuses_prompt_frames() {
    let mut queue = ReservedQueue::new(4, 128, 1, 32).unwrap();
    assert_eq!(
        queue.try_push_prompt_with(8, || 1),
        Err(QueueError::ByteCapacity)
    );
}
