use super::*;

/// The workbench layout and the active Project, on the shared database.
pub(crate) trait WorkbenchStore {
    #[cfg(test)]
    fn bootstrap_snapshot(&self) -> AppResult<BootstrapSnapshot>;

    fn bootstrap_snapshot_with_catalog(
        &self,
        harnesses: &HarnessCatalog,
    ) -> AppResult<BootstrapSnapshot>;

    fn persist_project_layout(&self, request: &ProjectLayoutRequest) -> AppResult<()>;

    fn switch_project(&self, request: &ProjectSwitchRequest) -> AppResult<()>;
}

impl WorkbenchStore for Database {
    #[cfg(test)]
    fn bootstrap_snapshot(&self) -> AppResult<BootstrapSnapshot> {
        self.bootstrap_snapshot_with_catalog(&HarnessCatalog::default())
    }

    fn bootstrap_snapshot_with_catalog(
        &self,
        harnesses: &HarnessCatalog,
    ) -> AppResult<BootstrapSnapshot> {
        let active_project_id = self.repair_active_project()?;
        let projects = self.project_list()?;
        if projects.len() > MAX_PROJECTS as usize {
            return Err(AppError::ResourceLimit(format!(
                "project registry exceeds its {MAX_PROJECTS}-project limit"
            )));
        }
        let Some(project_id) = active_project_id else {
            let mut snapshot = empty_snapshot(projects);
            snapshot.harnesses = harnesses.definitions();
            return Ok(snapshot);
        };
        let project = self.project_summary(&project_id)?;
        let roots = self.root_records(&project_id)?;
        let (
            tabs,
            archived_chats,
            active_tab_id,
            selected_root_id,
            selected_resource,
            chat_id,
            session,
        ) = {
            let connection = self.connection();
            let restored = restore_layout(&connection, &project_id, &roots)?;
            let archived_chats = archived_chats_in(&connection, &project_id)?;
            let chat_id =
                load_chat_id(&connection, &project_id, restored.active_tab_id.as_deref())?;
            let session = load_session(&connection, &project_id, chat_id.as_deref())?;
            (
                restored.tabs,
                archived_chats,
                restored.active_tab_id,
                restored.selected_root_id,
                restored.selected_resource,
                chat_id,
                session,
            )
        };
        let messages = match &chat_id {
            Some(id) => self.messages_for_chat(id)?,
            None => Vec::new(),
        };
        let mut definitions = restore_harnesses(&self.connection(), &project_id, &tabs, harnesses)?;
        let selected_harness_id = match &session {
            Some((_, binding_id, _)) => {
                Some(harnesses.restore_binding(binding_id, &mut definitions))
            }
            None => harnesses.default_selected_harness_id(),
        };
        Ok(BootstrapSnapshot {
            transition_epoch: 0,
            projects,
            project: Some(project),
            tabs,
            archived_chats,
            active_tab_id,
            selected_root_id,
            selected_resource,
            harnesses: definitions,
            session_id: session.as_ref().map(|value| value.0.clone()),
            selected_harness_id,
            selected_model_id: session.and_then(|value| value.2),
            messages,
            repositories: ProjectRepositoryStatus {
                roots: Vec::new(),
                truncated: false,
                elapsed_ms: 0,
            },
            turn_changes: Vec::new(),
            recovered_buffers: Vec::new(),
            recovery_notice: None,
        })
    }

    fn persist_project_layout(&self, request: &ProjectLayoutRequest) -> AppResult<()> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction().db()?;
        let active = active_project_in(&transaction)?;
        if active.as_deref() != Some(request.project_id.as_str()) {
            return Err(AppError::InvalidRequest(
                "layout does not belong to the active project".into(),
            ));
        }
        persist_layout_in(&transaction, request)?;
        transaction.commit().db()?;
        Ok(())
    }

    fn switch_project(&self, request: &ProjectSwitchRequest) -> AppResult<()> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction().db()?;
        let active = active_project_in(&transaction)?;
        if active.as_deref() != Some(request.current_layout.project_id.as_str()) {
            return Err(AppError::InvalidRequest(
                "project changed before the requested transition".into(),
            ));
        }
        persist_layout_in(&transaction, &request.current_layout)?;
        let target_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(
               SELECT 1 FROM projects p
               WHERE p.id = ?1
                 AND EXISTS(
                   SELECT 1 FROM project_roots r WHERE r.project_id = p.id
                 )
             )",
                [&request.target_project_id],
                |row| row.get(0),
            )
            .db()?;
        if !target_exists {
            return Err(AppError::InvalidRequest(
                "target project does not exist or has no roots".into(),
            ));
        }
        let now = now_millis();
        transaction
            .execute(
                "UPDATE app_state
             SET active_project_id = ?1, updated_at = ?2
             WHERE singleton = 1",
                params![request.target_project_id, now],
            )
            .db()?;
        transaction
            .execute(
                "UPDATE projects
             SET last_opened_at = ?2, updated_at = ?2
             WHERE id = ?1",
                params![request.target_project_id, now],
            )
            .db()?;
        transaction.commit().db()?;
        Ok(())
    }
}
