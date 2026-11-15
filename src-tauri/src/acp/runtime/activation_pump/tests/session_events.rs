use sage_acp_protocol::intent::session_event::{
    SessionNoticeLevel, SessionScopedEvent,
    fixtures::{notice as notice_fixture, title},
};

use super::*;

fn notice(text: &str) -> SessionScopedEvent {
    notice_fixture(SessionNoticeLevel::Info, text)
}

fn texts(events: Vec<SessionScopedEvent>) -> Vec<String> {
    events
        .into_iter()
        .map(|event| match event {
            SessionScopedEvent::Notice(notice) => notice.text.as_str().to_owned(),
            SessionScopedEvent::Title { title: Some(title) } => format!("title:{}", title.as_str()),
            other => panic!("unexpected session event {other:?}"),
        })
        .collect()
}

struct NoWrites;

impl crate::acp::runtime::EgressTransport for NoWrites {
    fn write(&mut self, _frame: &crate::acp::transport::OutboundFrame) -> FrameWriteOutcome {
        panic!("session events never write to the harness")
    }
}

#[test]
fn session_events_during_activation_are_buffered_until_the_session_identity_is_known() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let epoch = RuntimeEpoch::new(7).unwrap();
        let session = |event| AdapterEvent::Session {
            id: identity("external-1", epoch),
            event,
        };
        let foreign = AdapterEvent::Session {
            id: identity("external-other", epoch),
            event: notice("foreign session"),
        };
        let batches = vec![
            AdapterBatch::new(
                vec![
                    AdapterEvent::Initialized {
                        capabilities: advertised.clone(),
                    },
                    session(notice("before session ready")),
                    foreign,
                ],
                vec![],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![
                    session(title("Draft")),
                    AdapterEvent::SessionReady {
                        id: identity("external-1", epoch),
                        loaded: false,
                    },
                    session(title("Final")),
                ],
                vec![],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![
                    session(notice("idle before first turn")),
                    AdapterEvent::Session {
                        id: identity("external-1", RuntimeEpoch::new(8).unwrap()),
                        event: notice("stale epoch"),
                    },
                ],
                vec![],
            )
            .unwrap(),
        ];
        let (journal, commands) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(9).unwrap(),
        }));
        let mut pump = ActivationPump::new(
            adapter(advertised, batches),
            journal,
            NoResources(17),
            intent(ActivationMode::New {
                native_root: r"C:\repo".into(),
                model: None,
            }),
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.begin().unwrap();
        pump.flush_one(&mut transport).unwrap();
        push_frame(&mut pump, 10).await.unwrap();
        pump.flush_one(&mut transport).unwrap();
        push_frame(&mut pump, 11).await.unwrap();
        assert_eq!(commands.lock().unwrap().len(), 1);

        let mut idle = pump.try_into_idle_parts().ok().unwrap();
        let batch = idle.ingest_idle_frame(&frame()).unwrap();
        idle.accept_idle_batch(batch, &mut NoWrites).unwrap();
        assert_eq!(
            texts(idle.take_pending_session_events().take()),
            [
                "before session ready",
                "title:Final",
                "idle before first turn"
            ]
        );
        assert!(idle.take_pending_session_events().take().is_empty());
    });
}

#[test]
fn idle_parts_still_reject_turn_scoped_events_before_the_first_turn() {
    let advertised = capabilities(true, true);
    let activation = intent(ActivationMode::New {
        native_root: r"C:\repo".into(),
        model: None,
    });
    let session = ActivatedSession {
        binding_digest: activation.binding_digest,
        project_id: activation.project_id,
        chat_id: activation.chat_id,
        session_id: activation.session_id,
        external_session_id: ExternalId::new("thread", "external-1").unwrap(),
        runtime_epoch: activation.runtime_epoch,
        capabilities: advertised.clone(),
        models: Vec::new(),
        next_model_cursor: None,
    };
    let mut idle =
        IdleActivationParts::new(adapter(advertised, vec![]), (), NoResources(1), session);
    let turn_scoped = AdapterBatch::new(
        vec![AdapterEvent::Usage {
            turn_id: identity("external-1", activation.runtime_epoch),
            payload: NormalizedPayload::Usage {
                input_tokens: 1,
                output_tokens: 1,
                context_window: None,
            },
        }],
        vec![],
    )
    .unwrap();
    assert!(matches!(
        idle.accept_idle_batch(turn_scoped, &mut NoWrites),
        Err(crate::acp::runtime::AdapterPumpError::Frame)
    ));
}

fn frame() -> Frame {
    crate::acp::transport::IncrementalFrameDecoder::new()
        .push_chunk(b"{}\n")
        .unwrap()
        .unwrap()
}
