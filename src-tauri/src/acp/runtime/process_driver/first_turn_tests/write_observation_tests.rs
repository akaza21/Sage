use super::*;

struct WaitingClock(Mutex<Option<mpsc::SyncSender<()>>>);

impl DriverClock for WaitingClock {
    fn now_millis(&self) -> i64 {
        if let Some(waiting) = self.0.lock().unwrap().take() {
            waiting.send(()).unwrap();
        }
        SystemDriverClock.now_millis()
    }
}

#[test]
fn immediate_stdout_before_write_observation_completes_the_first_turn() {
    close_before_first_write(CloseBeforeWriteCase::EarlyStdoutBeforeObservation);
}

pub(super) async fn early_stdout_before_observation(
    state: FirstTurnWorkerState<Adapter, Mapper, Transport, NoChildProcessControl>,
    context: &mut FirstTurnWorkerContext<Authority>,
    database: &Database,
) {
    let FirstTurnWorkerState::Active(active) = state else {
        panic!("written prompt");
    };
    let turn_id = active.turn.dispatch.turn_id().clone();
    context.mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
    let mailbox = Arc::clone(&context.mailbox);
    let owner = context.owner.clone();
    let observed_turn = turn_id.clone();
    let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc);
    let (waiting, wait) = mpsc::sync_channel(1);
    context.clock = Arc::new(WaitingClock(Mutex::new(Some(waiting))));
    // Hold the observation until the real runner has entered its fenced wait.
    // No sleep, scheduler luck, or repeated prompt is needed to exercise the gap.
    let observer = std::thread::spawn(move || {
        wait.recv_timeout(budget).ok()?;
        let response = mailbox
            .submit_worker(WorkerCommand::ObserveSend {
                owner,
                turn_id: observed_turn,
                observation: crate::acp::store::SendObservation::CompleteBytes,
            })
            .ok()?;
        response.recv_timeout(budget).ok()
    });
    let step = super::super::live_runner::active_step_for_test(
        FirstTurnWorkerState::Active(active),
        context,
        SystemDriverClock.now_millis() + i64::try_from(budget.as_millis()).unwrap(),
    )
    .await;
    let observation = observer.join().unwrap();
    assert!(
        matches!(observation, Some(Ok(WorkerReply::Unit))),
        "write observation must remain deliverable after immediate stdout"
    );
    let Ok(FirstTurnWorkerState::Active(mut active)) = step else {
        panic!("early stdout must not terminate the runner before the observation");
    };
    active
        .turn
        .driver
        .run_next(context.mailbox.as_ref())
        .await
        .unwrap();
    let page = crate::acp::store::load_chat_page(
        &database.connection(),
        context.owner.project_id.clone(),
        context.owner.chat_id.clone(),
        None,
    )
    .unwrap();
    let turn = page
        .turns
        .as_slice()
        .iter()
        .find(|turn| turn.id == turn_id)
        .unwrap();
    assert_eq!(turn.state, TurnState::Completed);
}
