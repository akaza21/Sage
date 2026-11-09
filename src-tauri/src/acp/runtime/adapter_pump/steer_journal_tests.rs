use super::*;

fn accept(request_id: u64, accepted: bool) -> AdapterEvent {
    AdapterEvent::SteerResponse {
        request_id,
        accepted,
    }
}

/// The durable items in commit order: (external id, steered text or None).
fn journaled(shared: &Shared) -> Vec<(String, Option<String>)> {
    let state = shared.0.lock().unwrap();
    state
        .durable_batches
        .iter()
        .flatten()
        .filter_map(|event| match event {
            DurableAdapterEvent::Item {
                external_item_id,
                lifecycle,
                payload,
                ..
            } => Some((
                external_item_id.as_str().to_owned(),
                match payload {
                    NormalizedPayload::UserPrompt { text } => {
                        assert_eq!(*lifecycle, ItemLifecycle::Completed);
                        Some(text.as_str().to_owned())
                    }
                    _ => None,
                },
            )),
            _ => None,
        })
        .collect()
}

async fn steer(
    pump: &mut AdapterPump<FakeAdapter, FakeJournal, Mapper>,
    client_message_id: &str,
) -> u64 {
    let (_, request_id) = pump
        .queue_steer(
            Prompt::new("prompt", "Use the other file").unwrap(),
            RequiredText::new("client", client_message_id).unwrap(),
        )
        .unwrap();
    assert_eq!(
        pump.flush_one(&mut CountingTransport::default()).unwrap(),
        FlushDisposition::Written
    );
    request_id
}

#[test]
fn an_accepted_steer_journals_one_user_item_at_its_place_in_the_turn() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![item("before")], Vec::new()),
            owned(
                vec![accept(1, true), accept(1, true), item("after")],
                Vec::new(),
            ),
        ]);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        let request_id = steer(&mut pump, "steer-one").await;
        pump.push_stdout_chunk(b"{}\n", 2).await.unwrap();
        assert_eq!(pump.take_steer_response(request_id), Some(true));
        let steered = super::super::steer::steered_prompt_item(
            &Prompt::new("prompt", "Use the other file").unwrap(),
            &RequiredText::new("client", "steer-one").unwrap(),
        )
        .unwrap()
        .0;
        assert_eq!(
            journaled(&shared),
            [
                ("before".to_owned(), None),
                (
                    steered.as_str().to_owned(),
                    Some("Use the other file".to_owned())
                ),
                ("after".to_owned(), None),
            ]
        );
    });
}

#[test]
fn a_rejected_stale_or_unrequested_steer_accept_journals_nothing() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![accept(1, true)], Vec::new()),
            owned(vec![accept(9, true), accept(1, false)], Vec::new()),
            owned(vec![accept(1, true), item("after")], Vec::new()),
        ]);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        let request_id = steer(&mut pump, "steer-two").await;
        pump.push_stdout_chunk(b"{}\n", 2).await.unwrap();
        assert_eq!(pump.take_steer_response(request_id), Some(false));
        pump.push_stdout_chunk(b"{}\n", 3).await.unwrap();
        assert_eq!(journaled(&shared), [("after".to_owned(), None)]);
    });
}

#[test]
fn a_retried_steer_keeps_the_identity_of_its_client_message() {
    let prompt = Prompt::new("prompt", "Again").unwrap();
    let id = |value: &str| {
        super::super::steer::steered_prompt_item(
            &prompt,
            &RequiredText::new("client", value).unwrap(),
        )
        .unwrap()
        .0
    };
    assert_eq!(id("steer-one"), id("steer-one"));
    assert_ne!(id("steer-one"), id("steer-two"));
    let longest = "x".repeat(512);
    assert!(id(&longest).as_str().len() <= 512);
}
