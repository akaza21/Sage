use super::{
    MutationDisposition, prepare_turn,
    tests::{command, setup},
};

#[test]
fn prepare_commits_turn_user_item_and_send_intent_atomically() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    assert_eq!(prepared.disposition, MutationDisposition::Applied);
    for table in ["turns", "items", "session_facts"] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
    let duplicate = prepare_turn(&mut connection, command("hello")).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert_eq!(duplicate.turn_id, prepared.turn_id);
}
