use crate::acp::contract::{SageProjectId, SageRootId};

use super::StoreError;

pub(super) fn load_project_roots(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
) -> Result<Vec<SageRootId>, StoreError> {
    let mut statement = connection
        .prepare("SELECT id FROM project_roots WHERE project_id=?1 ORDER BY ordinal,id LIMIT 9")?;
    let mut roots = Vec::with_capacity(8);
    for row in statement.query_map([project.as_str()], |row| row.get::<_, String>(0))? {
        if roots.len() == 8 {
            return Err(StoreError::Corrupt("project root limit+1"));
        }
        roots.push(SageRootId::parse(row?).map_err(|_| StoreError::Corrupt("project root id"))?);
    }
    if roots.is_empty() {
        return Err(StoreError::Corrupt("project root membership"));
    }
    Ok(roots)
}
