use super::*;

pub(in crate::acp::migration) fn apply_v17(connection: &mut Connection) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        apply_v17_steps(&transaction)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (17, ?1)",
            [now_millis()],
        )?;
        transaction.pragma_update(None, "user_version", 17)?;
        verify_v8_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_reached(connection, SchemaClass::CurrentV17)
}

pub(in crate::acp::migration) fn apply_v17_steps(connection: &Connection) -> AppResult<()> {
    let original: String = connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE type='table' AND name='items'",
        [],
        |row| row.get(0),
    )?;
    let old_kind = "'text', 'thoughtSummary', 'plan', 'toolCall', 'toolUpdateResult',";
    let new_kind =
        "'text', 'thoughtSummary', 'plan', 'compactionNotice', 'toolCall', 'toolUpdateResult',";
    if original.matches(old_kind).count() != 1 || !original.starts_with("CREATE TABLE items (") {
        return invalid("v17 item table source is not the exact admitted shape");
    }
    let replacement = original
        .replacen("CREATE TABLE items (", "CREATE TABLE items_v17 (", 1)
        .replacen(old_kind, new_kind, 1);
    let schema_objects = connection
        .prepare("SELECT name, sql FROM sqlite_schema WHERE tbl_name='items' AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let names = schema_objects
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    if names
        != [
            "idx_items_owner",
            "ux_items_external_epoch_kind",
            "deny_item_delete",
            "deny_item_invalid_transition",
            "deny_terminal_item_mutation",
            "deny_tool_invalid_transition",
            "release_deleted_item_usage",
        ]
    {
        return invalid("v17 item indexes or triggers are incomplete");
    }
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))?;
    connection.execute_batch(&replacement)?;
    connection.execute_batch("INSERT INTO items_v17 SELECT * FROM items; DROP TABLE items; ALTER TABLE items_v17 RENAME TO items;")?;
    for (_, sql) in schema_objects {
        connection.execute_batch(&sql)?;
    }
    let copied: i64 = connection.query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))?;
    if copied != count {
        return invalid("v17 item rebuild changed its row count");
    }
    Ok(())
}
