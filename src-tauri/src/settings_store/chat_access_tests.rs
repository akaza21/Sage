use super::*;
use crate::acp_protocol::access_mode::SessionAccessMode::{
    AcceptEdits, Ask, Auto, FullAccess, Plan,
};

fn store() -> SettingsStore {
    SettingsStore::in_memory().unwrap()
}

#[test]
fn a_confirmed_choice_persists_for_its_chat_only() {
    let directory = std::env::temp_dir().join(format!("sage-chat-access-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("settings.sqlite");
    let store = SettingsStore::open(&path).unwrap();
    let token = store
        .prepare_chat_access("session-a", FullAccess, 1_000)
        .unwrap();
    store
        .grant_chat_access("session-a", FullAccess, &token, 2_000)
        .unwrap();
    store.set_chat_access("session-b", Plan, 2_000).unwrap();
    drop(store);
    let store = SettingsStore::open(&path).unwrap();
    assert_eq!(store.chat_access("session-a").unwrap(), Some(FullAccess));
    assert_eq!(store.chat_access("session-b").unwrap(), Some(Plan));
    assert_eq!(store.chat_access("session-c").unwrap(), None);
    // Clearing is idempotent.
    assert!(store.clear_chat_access("session-a").unwrap());
    assert!(!store.clear_chat_access("session-a").unwrap());
    assert_eq!(store.chat_access("session-a").unwrap(), None);
    // Windows refuses to delete a database that is still open.
    drop(store);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn permissive_modes_need_a_token_and_others_replace_them() {
    let store = store();
    for mode in [FullAccess, Auto] {
        assert!(store.set_chat_access("session-a", mode, 1).is_err());
    }
    for mode in [Ask, AcceptEdits, Plan] {
        assert!(store.prepare_chat_access("session-a", mode, 1).is_err());
    }
    let token = store.prepare_chat_access("session-a", Auto, 1).unwrap();
    store
        .grant_chat_access("session-a", Auto, &token, 2)
        .unwrap();
    assert_eq!(store.chat_access("session-a").unwrap(), Some(Auto));
    // Choosing a narrower mode leaves the permissive one server-side.
    store.set_chat_access("session-a", Ask, 3).unwrap();
    assert_eq!(store.chat_access("session-a").unwrap(), Some(Ask));
}

#[test]
fn a_replayed_stale_foreign_or_mismatched_token_never_grants() {
    let store = store();
    // No confirmation at all.
    assert!(
        store
            .grant_chat_access("session-a", FullAccess, "x", 1)
            .is_err()
    );
    let token = store.prepare_chat_access("session-a", Auto, 1_000).unwrap();
    // The token is bound to its mode.
    assert!(
        store
            .grant_chat_access("session-a", FullAccess, &token, 1_001)
            .is_err()
    );
    let token = store
        .prepare_chat_access("session-a", FullAccess, 1_000)
        .unwrap();
    // Another session cannot spend it.
    assert!(
        store
            .grant_chat_access("session-b", FullAccess, &token, 1_001)
            .is_err()
    );
    // It expires at its deadline, not a moment after.
    assert!(
        store
            .grant_chat_access(
                "session-a",
                FullAccess,
                &token,
                1_000 + ACCESS_CONFIRMATION_MILLIS
            )
            .is_err()
    );
    assert_eq!(store.chat_access("session-a").unwrap(), None);
    // A newer dialog replaces an older token.
    let older = store
        .prepare_chat_access("session-a", FullAccess, 2_000)
        .unwrap();
    let newer = store
        .prepare_chat_access("session-a", FullAccess, 2_001)
        .unwrap();
    assert!(
        store
            .grant_chat_access("session-a", FullAccess, &older, 2_002)
            .is_err()
    );
    store
        .grant_chat_access("session-a", FullAccess, &newer, 2_002)
        .unwrap();
    // A replay of the spent token fails even after the chat steps down.
    store.set_chat_access("session-a", Ask, 2_003).unwrap();
    assert!(
        store
            .grant_chat_access("session-a", FullAccess, &newer, 2_004)
            .is_err()
    );
    assert_eq!(store.chat_access("session-a").unwrap(), Some(Ask));
    // Stepping down also voids an open confirmation.
    let open = store
        .prepare_chat_access("session-a", FullAccess, 3_000)
        .unwrap();
    store.set_chat_access("session-a", Plan, 3_001).unwrap();
    assert!(
        store
            .grant_chat_access("session-a", FullAccess, &open, 3_002)
            .is_err()
    );
}

#[test]
fn choices_and_confirmations_stay_bounded() {
    let store = store();
    for index in 0..(MAX_CHAT_ACCESS_CHOICES + 3) {
        store
            .set_chat_access(&format!("session-{index}"), Plan, index as i64)
            .unwrap();
    }
    let kept: i64 = store
        .lock()
        .query_row("SELECT COUNT(*) FROM chat_access_choices", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(kept as usize, MAX_CHAT_ACCESS_CHOICES);
    assert_eq!(store.chat_access("session-0").unwrap(), None);
    for index in 0..(MAX_PENDING_CONFIRMATIONS + 4) {
        store
            .prepare_chat_access(
                &format!("pending-{index}"),
                FullAccess,
                10_000 + index as i64,
            )
            .unwrap();
    }
    let pending: i64 = store
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM chat_access_confirmations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending as usize, MAX_PENDING_CONFIRMATIONS);
    assert!(store.prepare_chat_access("", FullAccess, 1).is_err());
}

#[test]
fn an_unknown_stored_mode_reads_as_the_default() {
    let store = store();
    store
        .lock()
        .execute(
            "INSERT INTO chat_access_choices(session_id,mode,chosen_at,sequence)
             VALUES ('session-a','futureMode',1,1)",
            [],
        )
        .unwrap();
    let (mode, captured) =
        sage_test_support::test_diagnostics::capture_diagnostics(|| store.chat_access("session-a"));
    assert_eq!(mode.unwrap(), None);
    assert!(captured.contains("Unknown stored chat access mode"));
}

#[test]
fn choices_of_deleted_sessions_are_forgotten() {
    let store = store();
    let token = store.prepare_chat_access("gone", FullAccess, 1).unwrap();
    store
        .grant_chat_access("gone", FullAccess, &token, 2)
        .unwrap();
    store.set_chat_access("kept", Plan, 3).unwrap();
    store.prepare_chat_access("pending-gone", Auto, 4).unwrap();
    let forgotten = store
        .retain_chat_access(|session| Ok(session == "kept"))
        .unwrap();
    assert_eq!(forgotten, 1);
    assert_eq!(store.chat_access("gone").unwrap(), None);
    assert_eq!(store.chat_access("kept").unwrap(), Some(Plan));
    let pending: i64 = store
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM chat_access_confirmations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0);
    // A failing existence check leaves everything in place.
    assert!(
        store
            .retain_chat_access(|_| Err(AppError::InvalidRequest("busy".into())))
            .is_err()
    );
    assert_eq!(store.chat_access("kept").unwrap(), Some(Plan));
}

#[test]
fn a_pending_token_is_checked_without_being_spent() {
    let store = store();
    let token = store
        .prepare_chat_access("session-a", FullAccess, 1_000)
        .unwrap();
    let pending = |session: &str, mode, token: &str, now| {
        store
            .chat_access_pending(session, mode, token, now)
            .unwrap()
    };
    assert!(pending("session-a", FullAccess, &token, 1_500));
    // The check leaves the token for the grant.
    assert!(pending("session-a", FullAccess, &token, 1_500));
    assert!(!pending("session-a", Auto, &token, 1_500));
    assert!(!pending("session-b", FullAccess, &token, 1_500));
    assert!(!pending("session-a", FullAccess, "other", 1_500));
    let expiry = 1_000 + ACCESS_CONFIRMATION_MILLIS;
    assert!(!pending("session-a", FullAccess, &token, expiry));
    store
        .grant_chat_access("session-a", FullAccess, &token, 2_000)
        .unwrap();
    assert!(!pending("session-a", FullAccess, &token, 2_000));
}
