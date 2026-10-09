use super::*;

pub(in crate::acp::migration) fn apply_v17(connection: &mut Connection) -> AppResult<()> {
    apply_item_kind_version(connection, 17, SchemaClass::CurrentV17, apply_v17_steps)
}

pub(in crate::acp::migration) fn apply_through_v18(connection: &mut Connection) -> AppResult<()> {
    super::child_thread::apply_through_v17(connection)?;
    apply_v18(connection)
}

pub(in crate::acp::migration) fn build_fresh_v18(connection: &mut Connection) -> AppResult<()> {
    super::child_thread::build_fresh_v17(connection)?;
    apply_v18(connection)
}

pub(in crate::acp::migration) fn apply_v18(connection: &mut Connection) -> AppResult<()> {
    apply_item_kind_version(connection, 18, SchemaClass::CurrentV18, apply_v18_steps)
}

fn apply_item_kind_version(
    connection: &mut Connection,
    version: i64,
    schema: SchemaClass,
    steps: fn(&Connection) -> AppResult<()>,
) -> AppResult<()> {
    with_foreign_keys_disabled(connection, |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        steps(&transaction)?;
        #[cfg(test)]
        if version == 18 {
            super::super::test_failpoint::hit("v17-to-v18-after-schema")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
            [version, now_millis()],
        )?;
        transaction.pragma_update(None, "user_version", version)?;
        #[cfg(test)]
        if version == 18 {
            super::super::test_failpoint::hit("v17-to-v18-after-markers")?;
        }
        verify_v8_foreign_keys(&transaction)?;
        transaction.commit()?;
        Ok(())
    })?;
    verify_reached(connection, schema)
}

pub(in crate::acp::migration) fn apply_v17_steps(connection: &Connection) -> AppResult<()> {
    rebuild_items(
        connection,
        "CREATE TABLE items (",
        "items_v17",
        "'text', 'thoughtSummary', 'plan', 'toolCall', 'toolUpdateResult',",
        "'text', 'thoughtSummary', 'plan', 'compactionNotice', 'toolCall', 'toolUpdateResult',",
    )
}

pub(in crate::acp::migration) fn apply_v18_steps(connection: &Connection) -> AppResult<()> {
    rebuild_items(
        connection,
        "CREATE TABLE \"items\" (",
        "items_v18",
        "'text', 'thoughtSummary', 'plan', 'compactionNotice', 'toolCall', 'toolUpdateResult',",
        "'text', 'thoughtSummary', 'plan', 'compactionNotice', 'image', 'toolCall', 'toolUpdateResult',",
    )
}

fn rebuild_items(
    connection: &Connection,
    original_prefix: &str,
    replacement_table: &str,
    old_kind: &str,
    new_kind: &str,
) -> AppResult<()> {
    let original: String = connection.query_row(
        "SELECT sql FROM sqlite_schema WHERE type='table' AND name='items'",
        [],
        |row| row.get(0),
    )?;
    if original.matches(old_kind).count() != 1 || !original.starts_with(original_prefix) {
        return invalid("item table source is not the exact admitted shape");
    }
    let replacement = original
        .replacen(
            original_prefix,
            &format!("CREATE TABLE {replacement_table} ("),
            1,
        )
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
        return invalid("item indexes or triggers are incomplete");
    }
    let count: i64 = connection.query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))?;
    connection.execute_batch(&replacement)?;
    connection.execute_batch(&format!(
        "INSERT INTO {replacement_table} SELECT * FROM items; DROP TABLE items; ALTER TABLE {replacement_table} RENAME TO items;"
    ))?;
    for (_, sql) in schema_objects {
        connection.execute_batch(&sql)?;
    }
    let copied: i64 = connection.query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))?;
    if copied != count {
        return invalid("item rebuild changed its row count");
    }
    Ok(())
}
