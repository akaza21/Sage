use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterIdentity, EgressToken},
    contract::{RequiredText, SafeText, SchemaVersionV1},
    intent::{NormalizedPayload, SafeResourceRef},
    transport::Frame,
};

use super::*;

#[path = "tests/model_catalog.rs"]
mod model_catalog;

#[path = "tests/session_events.rs"]
mod session_events;

#[path = "tests/initialization.rs"]
mod initialization;

struct FakeAdapter {
    capabilities: CapabilitySnapshotV1,
    batches: VecDeque<AdapterBatch>,
    resolutions: Arc<Mutex<Vec<EgressResolution>>>,
    next_request: u64,
}

impl HarnessAdapter for FakeAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        Some(self.capabilities.clone())
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !matches!(
            command,
            AdapterCommand::Initialize
                | AdapterCommand::NewSession { .. }
                | AdapterCommand::ResumeSession { .. }
                | AdapterCommand::LoadSession { .. }
        ) {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        let id = self.next_request;
        self.next_request += 1;
        Ok(AdapterEgress::new(
            crate::acp::transport::OutboundFrame::new(format!("{{\"id\":{id}}}").into_bytes())
                .unwrap(),
            EgressToken::request(id),
        ))
    }

    fn ingest(
        &mut self,
        _frame: &Frame,
        _resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.batches.pop_front().ok_or(AdapterRuntimeError::Frame)
    }

    fn commit_mandatory_egress(&mut self, _token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        Err(AdapterRuntimeError::EgressIntent)
    }

    fn resolve_egress(
        &mut self,
        _token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.resolutions.lock().unwrap().push(resolution);
        Ok(())
    }
}

struct NoResources(u8);

impl ResourceMapper for NoResources {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}

#[derive(Clone)]
struct JournalState {
    commands: Arc<Mutex<Vec<CompleteActivationCommand>>>,
    outcome: Arc<Mutex<Option<Result<ActivationReceipt, ActivationJournalError>>>>,
}

impl ActivationJournal for JournalState {
    fn commit_ready<'a>(
        &'a mut self,
        command: CompleteActivationCommand,
    ) -> Pin<Box<dyn Future<Output = Result<ActivationReceipt, ActivationJournalError>> + Send + 'a>>
    {
        self.commands.lock().unwrap().push(command);
        let outcome = self.outcome.lock().unwrap().take().unwrap();
        Box::pin(async move { outcome })
    }
}

struct Transport(VecDeque<FrameWriteOutcome>);

impl ActivationTransport for Transport {
    fn write(&mut self, _frame: &crate::acp::transport::OutboundFrame) -> FrameWriteOutcome {
        self.0.pop_front().unwrap_or(FrameWriteOutcome::Complete)
    }
}

#[test]
fn initialize_new_session_commits_exact_effective_identity_before_ready() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let notification = AdapterEgress::new(
            crate::acp::transport::OutboundFrame::new(b"{\"method\":\"initialized\"}".to_vec())
                .unwrap(),
            EgressToken::protocol_notification(),
        );
        let model = AdapterModel {
            id: ModelId::new("modelId", "m1").unwrap(),
            name: RequiredText::new("modelName", "Model One").unwrap(),
            is_default: true,
        };
        let batches = vec![
            AdapterBatch::new(
                vec![
                    AdapterEvent::Initialized {
                        capabilities: advertised.clone(),
                    },
                    AdapterEvent::Models {
                        models: vec![model.clone()],
                        next_cursor: None,
                    },
                    AdapterEvent::IgnoredLate,
                ],
                vec![notification],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionReady {
                    id: identity("external-1", RuntimeEpoch::new(7).unwrap()),
                    loaded: false,
                }],
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
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        push_frame(&mut pump, 10).await.unwrap();
        assert!(
            commands.lock().unwrap().is_empty(),
            "published before durable activation"
        );
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        push_frame(&mut pump, 11).await.unwrap();

        let parts = pump.try_into_idle_parts().ok().unwrap();
        assert_eq!(parts.session().external_session_id.as_str(), "external-1");
        let (adapter, returned_journal, resources, ready) = parts.into_parts();
        assert_eq!(ready.external_session_id.as_str(), "external-1");
        assert_eq!(ready.models, vec![model]);
        assert!(ready.capabilities.prompt);
        assert!(
            !ready.capabilities.cancel,
            "policy intersection must deny cancel"
        );
        assert_eq!(
            adapter.next_request, 3,
            "the same live adapter was returned"
        );
        assert_eq!(resources.0, 17, "the same native mapper was returned");
        assert!(Arc::ptr_eq(&returned_journal.commands, &commands));
        let committed = commands.lock().unwrap();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].session_id, session_id());
        assert_eq!(committed[0].runtime_epoch.get(), 7);
        assert_eq!(committed[0].external_session_id.as_str(), "external-1");
        assert!(committed[0].capabilities_json.contains("\"cancel\":false"));
    });
}

#[test]
fn prompt_capability_stays_enabled_without_a_model_catalog() {
    runtime().block_on(async {
        let mut advertised = capabilities(true, true);
        advertised.model_count = 0;
        let notification = AdapterEgress::new(
            crate::acp::transport::OutboundFrame::new(b"{\"method\":\"initialized\"}".to_vec())
                .unwrap(),
            EgressToken::protocol_notification(),
        );
        let batches = vec![
            AdapterBatch::new(
                vec![AdapterEvent::Initialized {
                    capabilities: advertised.clone(),
                }],
                vec![notification],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionReady {
                    id: identity("external-no-models", RuntimeEpoch::new(7).unwrap()),
                    loaded: false,
                }],
                vec![],
            )
            .unwrap(),
        ];
        let (journal, _commands) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(9).unwrap(),
        }));
        let mut activation_intent = intent(ActivationMode::New {
            native_root: r"C:\repo".into(),
            model: None,
        });
        activation_intent.curated_ceiling.model_count = 0;
        activation_intent.sage_policy.model_count = 0;
        let mut pump = ActivationPump::new(
            adapter(advertised, batches),
            journal,
            NoResources(17),
            activation_intent,
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.begin().unwrap();
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        push_frame(&mut pump, 10).await.unwrap();
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        push_frame(&mut pump, 11).await.unwrap();

        let parts = pump.try_into_idle_parts().ok().unwrap();
        let (_, _, _, ready) = parts.into_parts();
        assert!(ready.models.is_empty());
        assert_eq!(ready.capabilities.model_count, 0);
        assert!(ready.capabilities.prompt);
    });
}

#[test]
fn available_commands_update_requires_the_activated_session_identity() {
    let mut decoder = crate::acp::transport::IncrementalFrameDecoder::new();
    let frame = decoder
        .push_chunk(
            b"{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"external-1\",\"update\":{\"sessionUpdate\":\"available_commands_update\",\"availableCommands\":[]}}}\n",
        )
        .unwrap()
        .unwrap();

    assert!(is_available_commands_update(&frame, "external-1"));
    assert!(!is_available_commands_update(&frame, "foreign-session"));

    let missing_jsonrpc = decoder
        .push_chunk(
            b"{\"method\":\"session/update\",\"params\":{\"sessionId\":\"external-1\",\"update\":{\"sessionUpdate\":\"available_commands_update\",\"availableCommands\":[]}}}\n",
        )
        .unwrap()
        .unwrap();
    assert!(!is_available_commands_update(
        &missing_jsonrpc,
        "external-1"
    ));

    let mixed_response = decoder
        .push_chunk(
            b"{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"result\":{},\"params\":{\"sessionId\":\"external-1\",\"update\":{\"sessionUpdate\":\"available_commands_update\",\"availableCommands\":[]}}}\n",
        )
        .unwrap()
        .unwrap();
    assert!(!is_available_commands_update(&mixed_response, "external-1"));
}

#[test]
fn an_absorbed_harness_state_notification_between_initialize_and_session_does_not_fail_activation()
{
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let notification = AdapterEgress::new(
            crate::acp::transport::OutboundFrame::new(b"{\"method\":\"initialized\"}".to_vec())
                .unwrap(),
            EgressToken::protocol_notification(),
        );
        let batches = vec![
            AdapterBatch::new(
                vec![AdapterEvent::Initialized {
                    capabilities: advertised.clone(),
                }],
                vec![notification],
            )
            .unwrap(),
            AdapterBatch::new(vec![AdapterEvent::Absorbed], vec![]).unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionReady {
                    id: identity("external-1", RuntimeEpoch::new(7).unwrap()),
                    loaded: false,
                }],
                vec![],
            )
            .unwrap(),
        ];
        let (journal, _commands) = journal(Ok(ActivationReceipt {
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
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        push_frame(&mut pump, 10).await.unwrap();
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            ActivationFlush::Written
        );
        push_frame(&mut pump, 11)
            .await
            .expect("an absorbed harness state notification is not a protocol error");
        push_frame(&mut pump, 12).await.unwrap();
        let parts = pump.try_into_idle_parts().ok().unwrap();
        assert_eq!(parts.session().external_session_id.as_str(), "external-1");
    });
}

#[test]
fn resume_and_load_bind_only_the_persisted_external_session() {
    runtime().block_on(async {
        for (mode, loaded) in [
            (
                ActivationMode::Resume {
                    external_session_id: ExternalId::new("externalSessionId", "persisted").unwrap(),
                    native_root: "/repo".into(),
                    model: None,
                },
                false,
            ),
            (
                ActivationMode::Load {
                    external_session_id: ExternalId::new("externalSessionId", "persisted").unwrap(),
                },
                true,
            ),
        ] {
            let advertised = capabilities(true, true);
            let batches = vec![
                AdapterBatch::new(
                    vec![AdapterEvent::Initialized {
                        capabilities: advertised.clone(),
                    }],
                    vec![],
                )
                .unwrap(),
                AdapterBatch::new(
                    vec![AdapterEvent::SessionReady {
                        id: identity("persisted", RuntimeEpoch::new(7).unwrap()),
                        loaded,
                    }],
                    vec![],
                )
                .unwrap(),
            ];
            let (journal, commands) = journal(Ok(ActivationReceipt {
                projection_revision: ProjectionRevision::new(3).unwrap(),
            }));
            let mut pump = ActivationPump::new(
                adapter(advertised, batches),
                journal,
                NoResources(17),
                intent(mode),
            )
            .unwrap();
            let mut transport = Transport(VecDeque::new());
            pump.begin().unwrap();
            pump.flush_one(&mut transport).unwrap();
            push_frame(&mut pump, 1).await.unwrap();
            pump.flush_one(&mut transport).unwrap();
            push_frame(&mut pump, 2).await.unwrap();
            let parts = pump.try_into_idle_parts().ok().unwrap();
            assert_eq!(parts.session().external_session_id.as_str(), "persisted");
            assert_eq!(commands.lock().unwrap().len(), 1);
        }
    });
}

#[test]
fn foreign_session_ready_is_rejected_without_durable_completion() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let mut foreign = identity("external-1", RuntimeEpoch::new(8).unwrap());
        foreign.session_id = SageAgentSessionId::parse("foreign-session").unwrap();
        let batches = vec![
            AdapterBatch::new(
                vec![AdapterEvent::Initialized {
                    capabilities: advertised.clone(),
                }],
                vec![],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionReady {
                    id: foreign,
                    loaded: false,
                }],
                vec![],
            )
            .unwrap(),
        ];
        let (journal, commands) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(1).unwrap(),
        }));
        let mut pump = ActivationPump::new(
            adapter(advertised, batches),
            journal,
            NoResources(17),
            intent_with_epoch(
                ActivationMode::New {
                    native_root: "/repo".into(),
                    model: None,
                },
                8,
            ),
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.begin().unwrap();
        pump.flush_one(&mut transport).unwrap();
        push_frame(&mut pump, 1).await.unwrap();
        pump.flush_one(&mut transport).unwrap();
        assert!(matches!(
            push_frame(&mut pump, 2).await,
            Err(ActivationPumpError::Intent)
        ));
        assert!(pump.teardown_required());
        assert!(commands.lock().unwrap().is_empty());
        let teardown = pump.try_into_teardown_parts().ok().unwrap();
        assert_eq!(teardown.adapter.batches.len(), 0);
        assert_eq!(teardown.resources.0, 17);
        assert!(Arc::ptr_eq(&teardown.journal.commands, &commands));
    });
}

#[test]
fn malformed_and_unknown_activation_frames_fail_closed() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let (malformed_journal, _) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(1).unwrap(),
        }));
        let mut malformed = ActivationPump::new(
            adapter(advertised.clone(), vec![]),
            malformed_journal,
            NoResources(17),
            intent(ActivationMode::New {
                native_root: "/repo".into(),
                model: None,
            }),
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        malformed.begin().unwrap();
        malformed.flush_one(&mut transport).unwrap();
        assert!(matches!(
            malformed.push_stdout_chunk(b"[]\n", 1).await,
            Err(ActivationPumpError::Frame)
        ));
        assert!(malformed.teardown_required());

        let unknown = AdapterBatch::new(
            vec![AdapterEvent::Unsupported(NormalizedPayload::Unsupported {
                external_kind: SafeText::new("externalKind", "mystery").unwrap(),
                summary: SafeText::new("summary", "unsupported during activation").unwrap(),
            })],
            vec![],
        )
        .unwrap();
        let (journal, commands) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(1).unwrap(),
        }));
        let mut unknown_pump = ActivationPump::new(
            adapter(advertised, vec![unknown]),
            journal,
            NoResources(17),
            intent(ActivationMode::New {
                native_root: "/repo".into(),
                model: None,
            }),
        )
        .unwrap();
        unknown_pump.begin().unwrap();
        unknown_pump.flush_one(&mut transport).unwrap();
        assert!(matches!(
            push_frame(&mut unknown_pump, 2).await,
            Err(ActivationPumpError::Intent)
        ));
        assert!(unknown_pump.teardown_required());
        assert!(commands.lock().unwrap().is_empty());
    });
}

#[test]
fn models_limit_plus_one_fails_before_retaining_or_committing() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let model = AdapterModel {
            id: ModelId::new("modelId", "m").unwrap(),
            name: RequiredText::new("modelName", "model").unwrap(),
            is_default: false,
        };
        let exact = vec![model.clone(); LIMITS.wire.models_or_config_entries as usize];
        let exact_batch = AdapterBatch::new(
            vec![
                AdapterEvent::Initialized {
                    capabilities: advertised.clone(),
                },
                AdapterEvent::Models {
                    models: exact,
                    next_cursor: None,
                },
            ],
            vec![],
        )
        .unwrap();
        let (exact_journal, _) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(1).unwrap(),
        }));
        let mut exact_pump = ActivationPump::new(
            adapter(advertised.clone(), vec![exact_batch]),
            exact_journal,
            NoResources(17),
            intent(ActivationMode::New {
                native_root: "/repo".into(),
                model: None,
            }),
        )
        .unwrap();
        let mut exact_transport = Transport(VecDeque::new());
        exact_pump.begin().unwrap();
        exact_pump.flush_one(&mut exact_transport).unwrap();
        push_frame(&mut exact_pump, 1).await.unwrap();
        assert_eq!(
            exact_pump.models.len(),
            LIMITS.wire.models_or_config_entries as usize
        );
        assert!(!exact_pump.teardown_required());

        let over = vec![model; LIMITS.wire.models_or_config_entries as usize + 1];
        let batch = AdapterBatch::new(
            vec![
                AdapterEvent::Initialized {
                    capabilities: advertised.clone(),
                },
                AdapterEvent::Models {
                    models: over,
                    next_cursor: None,
                },
            ],
            vec![],
        )
        .unwrap();
        let (journal, commands) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(1).unwrap(),
        }));
        let mut pump = ActivationPump::new(
            adapter(advertised, vec![batch]),
            journal,
            NoResources(17),
            intent(ActivationMode::New {
                native_root: "/repo".into(),
                model: None,
            }),
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.begin().unwrap();
        pump.flush_one(&mut transport).unwrap();
        assert!(matches!(
            push_frame(&mut pump, 1).await,
            Err(ActivationPumpError::Capacity)
        ));
        assert!(pump.teardown_required());
        assert!(commands.lock().unwrap().is_empty());
    });
}

#[test]
fn zero_bytes_retries_once_but_possible_bytes_never_replays() {
    let advertised = capabilities(true, true);
    let (zero_journal, _) = journal(Ok(ActivationReceipt {
        projection_revision: ProjectionRevision::new(1).unwrap(),
    }));
    let resolutions = Arc::new(Mutex::new(Vec::new()));
    let mut pump = ActivationPump::new(
        FakeAdapter {
            capabilities: advertised,
            batches: VecDeque::new(),
            resolutions: Arc::clone(&resolutions),
            next_request: 1,
        },
        zero_journal,
        NoResources(17),
        intent(ActivationMode::New {
            native_root: "/repo".into(),
            model: None,
        }),
    )
    .unwrap();
    pump.begin().unwrap();
    let mut zero = Transport(VecDeque::from([
        FrameWriteOutcome::ZeroByteFailure,
        FrameWriteOutcome::ZeroByteFailure,
    ]));
    assert_eq!(
        pump.flush_one(&mut zero).unwrap(),
        ActivationFlush::RetriedZeroBytes
    );
    assert!(matches!(
        pump.flush_one(&mut zero),
        Err(ActivationPumpError::RetryExhausted)
    ));
    assert_eq!(
        *resolutions.lock().unwrap(),
        [
            EgressResolution::Queued,
            EgressResolution::WriteZeroBytes,
            EgressResolution::Queued,
            EgressResolution::WriteZeroBytes
        ]
    );

    let advertised = capabilities(true, true);
    let (uncertain_journal, _) = journal(Ok(ActivationReceipt {
        projection_revision: ProjectionRevision::new(1).unwrap(),
    }));
    let mut pump = ActivationPump::new(
        adapter(advertised, vec![]),
        uncertain_journal,
        NoResources(17),
        intent(ActivationMode::New {
            native_root: "/repo".into(),
            model: None,
        }),
    )
    .unwrap();
    pump.begin().unwrap();
    let mut uncertain = Transport(VecDeque::from([FrameWriteOutcome::DeliveryUnknown {
        bytes_written: 1,
    }]));
    assert_eq!(
        pump.flush_one(&mut uncertain).unwrap(),
        ActivationFlush::DeliveryUnknown
    );
    assert!(pump.teardown_required());
    assert!(matches!(
        pump.flush_one(&mut uncertain),
        Err(ActivationPumpError::TeardownRequired)
    ));
    let teardown = pump.try_into_teardown_parts().ok().unwrap();
    assert_eq!(teardown.resources.0, 17);
}

#[test]
fn ownership_handoff_waits_for_the_decoder_and_egress_to_be_quiescent() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let batches = vec![
            AdapterBatch::new(
                vec![AdapterEvent::Initialized {
                    capabilities: advertised.clone(),
                }],
                vec![],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionReady {
                    id: identity("external-1", RuntimeEpoch::new(7).unwrap()),
                    loaded: false,
                }],
                vec![],
            )
            .unwrap(),
            AdapterBatch::new(vec![AdapterEvent::IgnoredLate], vec![]).unwrap(),
        ];
        let (journal, _) = journal(Ok(ActivationReceipt {
            projection_revision: ProjectionRevision::new(4).unwrap(),
        }));
        let mut pump = ActivationPump::new(
            adapter(advertised, batches),
            journal,
            NoResources(29),
            intent(ActivationMode::New {
                native_root: "/repo".into(),
                model: None,
            }),
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.begin().unwrap();
        pump.flush_one(&mut transport).unwrap();
        push_frame(&mut pump, 1).await.unwrap();
        pump.flush_one(&mut transport).unwrap();

        assert!(pump.push_stdout_chunk(b"{}\n{}\n", 2).await.unwrap());
        assert!(
            !pump.completion_ready(),
            "a second decoded frame is retained"
        );
        let mut pump = match pump.try_into_idle_parts() {
            Err(error) => match *error {
                (pump, ActivationPumpError::NotQuiescent) => pump,
                _ => panic!("handoff must preserve ownership while input is pending"),
            },
            Ok(_) => panic!("handoff must preserve ownership while input is pending"),
        };
        assert!(pump.drain_stdout(3).await.unwrap());
        let parts = pump.try_into_idle_parts().ok().unwrap();
        let (adapter, _journal, resources, session) = parts.into_parts();
        assert_eq!(adapter.batches.len(), 0);
        assert_eq!(resources.0, 29);
        assert_eq!(session.external_session_id.as_str(), "external-1");
    });
}

#[test]
fn commit_unknown_blocks_for_authoritative_recovery_without_releasing_ownership() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let batches = vec![
            AdapterBatch::new(
                vec![AdapterEvent::Initialized {
                    capabilities: advertised.clone(),
                }],
                vec![],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionReady {
                    id: identity("external-1", RuntimeEpoch::new(7).unwrap()),
                    loaded: false,
                }],
                vec![],
            )
            .unwrap(),
        ];
        let (journal, commands) = journal(Err(ActivationJournalError::CommitUnknown));
        let mut pump = ActivationPump::new(
            adapter(advertised, batches),
            journal,
            NoResources(17),
            intent(ActivationMode::New {
                native_root: "/repo".into(),
                model: None,
            }),
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.begin().unwrap();
        pump.flush_one(&mut transport).unwrap();
        push_frame(&mut pump, 1).await.unwrap();
        pump.flush_one(&mut transport).unwrap();
        assert!(matches!(
            push_frame(&mut pump, 2).await,
            Err(ActivationPumpError::RecoveryRequired)
        ));
        assert_eq!(commands.lock().unwrap().len(), 1);
        assert!(pump.projection_recovery_required());
        assert!(!pump.teardown_required());
        assert!(!pump.completion_ready());
        let recovery = pump.try_into_recovery_parts().ok().unwrap();
        assert_eq!(recovery.adapter.batches.len(), 0);
        assert_eq!(recovery.resources.0, 17);
        assert!(Arc::ptr_eq(&recovery.journal.commands, &commands));
    });
}

#[test]
fn post_commit_bootstrap_failure_blocks_ready_projection() {
    runtime().block_on(async {
        let advertised = capabilities(true, true);
        let batches = vec![
            AdapterBatch::new(
                vec![AdapterEvent::Initialized {
                    capabilities: advertised.clone(),
                }],
                vec![],
            )
            .unwrap(),
            AdapterBatch::new(
                vec![AdapterEvent::SessionReady {
                    id: identity("external-1", RuntimeEpoch::new(7).unwrap()),
                    loaded: false,
                }],
                vec![],
            )
            .unwrap(),
        ];
        let (journal, commands) = journal(Err(ActivationJournalError::BootstrapRequired));
        let mut pump = ActivationPump::new(
            adapter(advertised, batches),
            journal,
            NoResources(17),
            intent(ActivationMode::New {
                native_root: "/repo".into(),
                model: None,
            }),
        )
        .unwrap();
        let mut transport = Transport(VecDeque::new());
        pump.begin().unwrap();
        pump.flush_one(&mut transport).unwrap();
        push_frame(&mut pump, 1).await.unwrap();
        pump.flush_one(&mut transport).unwrap();
        assert!(matches!(
            push_frame(&mut pump, 2).await,
            Err(ActivationPumpError::BootstrapRequired)
        ));
        assert_eq!(
            commands.lock().unwrap().len(),
            1,
            "durable commit occurred before projection failure"
        );
        assert!(pump.projection_recovery_required());
        assert!(!pump.completion_ready());
        assert!(matches!(
            pump.flush_one(&mut transport),
            Err(ActivationPumpError::RecoveryRequired)
        ));
        let recovery = pump.try_into_recovery_parts().ok().unwrap();
        assert_eq!(recovery.adapter.batches.len(), 0);
        assert_eq!(recovery.resources.0, 17);
        assert!(Arc::ptr_eq(&recovery.journal.commands, &commands));
    });
}

fn adapter(capabilities: CapabilitySnapshotV1, batches: Vec<AdapterBatch>) -> FakeAdapter {
    FakeAdapter {
        capabilities,
        batches: batches.into(),
        resolutions: Arc::new(Mutex::new(Vec::new())),
        next_request: 1,
    }
}

fn journal(
    result: Result<ActivationReceipt, ActivationJournalError>,
) -> (JournalState, Arc<Mutex<Vec<CompleteActivationCommand>>>) {
    let commands = Arc::new(Mutex::new(Vec::new()));
    (
        JournalState {
            commands: Arc::clone(&commands),
            outcome: Arc::new(Mutex::new(Some(result))),
        },
        commands,
    )
}

async fn push_frame(
    pump: &mut ActivationPump<FakeAdapter, JournalState, NoResources>,
    now: i64,
) -> Result<bool, ActivationPumpError> {
    pump.push_stdout_chunk(b"{}\n", now).await
}

fn identity(external: &str, epoch: RuntimeEpoch) -> AdapterIdentity {
    AdapterIdentity {
        binding_digest: [7; 32],
        runtime_epoch: epoch,
        session_id: session_id(),
        external_id: ExternalId::new("externalSessionId", external).unwrap(),
        turn_id: None,
        turn_client_message_id: None,
    }
}

fn intent(mode: ActivationMode) -> ActivationIntent {
    intent_with_epoch(mode, 7)
}

fn intent_with_epoch(mode: ActivationMode, epoch: u64) -> ActivationIntent {
    ActivationIntent {
        binding_change: None,
        binding_digest: [7; 32],
        artifact_digest: "ab".repeat(32),
        project_id: SageProjectId::parse("project-activation").unwrap(),
        chat_id: SageChatId::parse("chat-activation").unwrap(),
        session_id: session_id(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
        mode,
        curated_ceiling: capabilities(true, true),
        sage_policy: capabilities(true, false),
    }
}

fn session_id() -> SageAgentSessionId {
    SageAgentSessionId::parse("session-activation").unwrap()
}

fn capabilities(prompt: bool, cancel: bool) -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "acp-test-v1").unwrap(),
        new_session: true,
        resume_session: true,
        load_session: true,
        close_session: false,
        prompt,
        cancel,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 128,
        prompt_resources: false,
        config_count: 0,
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
