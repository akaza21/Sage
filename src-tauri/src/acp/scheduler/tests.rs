use super::*;

fn owner(project: &str, epoch: u64) -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse(project).unwrap(),
        chat_id: SageChatId::parse(format!("chat-{project}")).unwrap(),
        session_id: SageAgentSessionId::parse(format!("session-{project}")).unwrap(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
    }
}

#[test]
fn hot_ready_tree_and_running_turn_have_separate_slots() {
    let scheduler = Scheduler::new();
    let tree = scheduler.reserve_tree(owner("one", 1)).unwrap();
    assert!(
        scheduler
            .active()
            .is_some_and(|active| !active.turn_reserved)
    );
    assert_eq!(
        scheduler.reserve_tree(owner("two", 1)).unwrap_err(),
        SchedulerError::TreeCapacity
    );
    let turn = tree.reserve_turn().unwrap();
    assert_eq!(
        tree.reserve_turn().unwrap_err(),
        SchedulerError::TurnCapacity
    );
    turn.abort_before_durable_commit().unwrap();
    tree.zero_process_converged().unwrap();
}

#[test]
fn project_switch_keeps_both_background_slots_charged() {
    let scheduler = Scheduler::new();
    let original = owner("one", 7);
    let tree = scheduler.reserve_tree(original.clone()).unwrap();
    let turn = tree
        .reserve_turn()
        .unwrap()
        .bind(SageTurnId::parse("turn-one").unwrap())
        .unwrap();
    scheduler.project_switched(&SageProjectId::parse("two").unwrap());
    assert_eq!(scheduler.active().unwrap().tree, original);
    assert_eq!(
        scheduler.reserve_tree(owner("two", 1)).unwrap_err(),
        SchedulerError::TreeCapacity
    );
    turn.terminal_committed().unwrap();
    tree.zero_process_converged().unwrap();
}

#[test]
fn failed_zero_process_convergence_returns_the_exact_tree_lease_for_retry() {
    let scheduler = Scheduler::new();
    let tree = scheduler.reserve_tree(owner("one", 1)).unwrap();
    let turn = tree
        .reserve_turn()
        .unwrap()
        .bind(SageTurnId::parse("turn-retained-zero").unwrap())
        .unwrap();

    let (tree, error) = tree.zero_process_converged_retained().unwrap_err();
    assert_eq!(error, SchedulerError::TurnStillActive);
    assert_eq!(tree.owner(), &owner("one", 1));
    turn.terminal_committed().unwrap();
    tree.zero_process_converged_retained().unwrap();
    assert!(scheduler.active().is_none());
}

#[test]
fn dropped_running_lease_retains_capacity_until_recovery() {
    let scheduler = Scheduler::new();
    let tree = scheduler.reserve_tree(owner("one", 1)).unwrap();
    let running = tree
        .reserve_turn()
        .unwrap()
        .bind(SageTurnId::parse("turn-one").unwrap())
        .unwrap();
    drop(running);
    assert_eq!(
        tree.reserve_turn().unwrap_err(),
        SchedulerError::TurnCapacity
    );
    assert!(matches!(
        tree.zero_process_converged(),
        Err(SchedulerError::TurnStillActive)
    ));
}

#[test]
fn dropped_tree_lease_never_advertises_a_second_tree() {
    let scheduler = Scheduler::new();
    drop(scheduler.reserve_tree(owner("one", 1)).unwrap());
    assert_eq!(
        scheduler.reserve_tree(owner("two", 1)).unwrap_err(),
        SchedulerError::TreeCapacity
    );
}

#[test]
fn known_precommit_failure_can_release_only_the_turn_reservation() {
    let scheduler = Scheduler::new();
    let tree = scheduler.reserve_tree(owner("one", 1)).unwrap();
    tree.reserve_turn()
        .unwrap()
        .abort_before_durable_commit()
        .unwrap();
    assert!(tree.reserve_turn().is_ok());
}

#[test]
fn terminal_turn_releases_only_turn_slot_until_session_close_drains_tree() {
    let scheduler = Scheduler::new();
    let tree = scheduler.reserve_tree(owner("one", 1)).unwrap();
    tree.reserve_turn()
        .unwrap()
        .bind(SageTurnId::parse("turn-one").unwrap())
        .unwrap()
        .terminal_committed()
        .unwrap();

    let second = tree.reserve_turn().unwrap();
    second.abort_before_durable_commit().unwrap();
    tree.begin_draining().unwrap();
    assert_eq!(
        tree.reserve_turn().unwrap_err(),
        SchedulerError::TreeCapacity
    );
    tree.zero_process_converged().unwrap();
}
