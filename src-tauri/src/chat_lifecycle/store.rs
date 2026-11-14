use super::*;

/// Creating and renaming Chats, on the shared database.
pub(crate) trait ChatLifecycleStore {
    #[cfg(test)]
    fn create_chat(&self, current_layout: &ProjectLayoutRequest) -> AppResult<String>;

    fn create_chat_on(
        &self,
        project_id: &str,
        current_layout: &ProjectLayoutRequest,
    ) -> AppResult<String>;

    fn create_chat_with_parent(
        &self,
        project_id: &str,
        current_layout: &ProjectLayoutRequest,
        parent: Option<&str>,
    ) -> AppResult<String>;

    fn rename_chat(&self, request: &RenameChatRequest) -> AppResult<()>;
}

impl ChatLifecycleStore for Database {
    #[cfg(test)]
    fn create_chat(&self, current_layout: &ProjectLayoutRequest) -> AppResult<String> {
        self.create_chat_on(&current_layout.project_id, current_layout)
    }

    fn create_chat_on(
        &self,
        project_id: &str,
        current_layout: &ProjectLayoutRequest,
    ) -> AppResult<String> {
        self.create_chat_with_parent(project_id, current_layout, None)
    }

    fn create_chat_with_parent(
        &self,
        project_id: &str,
        current_layout: &ProjectLayoutRequest,
        parent: Option<&str>,
    ) -> AppResult<String> {
        validate_chat_identifier("Project ID", project_id)?;
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction().db()?;
        if let Some(parent) = parent {
            crate::side_chat_storage::validate_parent(&transaction, project_id, parent)?;
        }
        let active_project_id = transaction
            .query_row(
                "SELECT active_project_id FROM app_state WHERE singleton=1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .db()?;
        let source_is_active = active_project_id.as_deref() == Some(project_id);
        if source_is_active {
            if current_layout.tabs.len() >= MAX_RESTORED_TABS {
                return Err(AppError::ResourceLimit(format!(
                    "project layout has reached its {MAX_RESTORED_TABS}-tab limit"
                )));
            }
            if current_layout.project_id != project_id {
                return Err(AppError::InvalidRequest(
                    "Chat layout does not belong to the requested Project".into(),
                ));
            }
            persist_layout_in(&transaction, current_layout)?;
        } else {
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
                    [project_id],
                    |row| row.get(0),
                )
                .db()?;
            if !exists {
                return Err(AppError::InvalidRequest(
                    "Chat does not belong to that Project".into(),
                ));
            }
            let tab_count: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM workbench_tabs WHERE project_id=?1",
                    [project_id],
                    |row| row.get(0),
                )
                .db()?;
            if tab_count >= MAX_RESTORED_TABS as i64 {
                return Err(AppError::ResourceLimit(format!(
                    "project layout has reached its {MAX_RESTORED_TABS}-tab limit"
                )));
            }
        }
        let chat_id = insert_new_chat(&transaction, project_id, source_is_active, parent)?;
        transaction.commit().db()?;
        Ok(chat_id)
    }

    fn rename_chat(&self, request: &RenameChatRequest) -> AppResult<()> {
        crate::side_chat_storage::refuse_persistence(&self.connection(), &request.chat_id)?;
        validate_chat_identifier("Project ID", &request.project_id)?;
        validate_chat_identifier("Chat ID", &request.chat_id)?;
        let title = canonical_chat_title(&request.title)?;
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction().db()?;
        let active_project_id = transaction
            .query_row(
                "SELECT active_project_id FROM app_state WHERE singleton=1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .db()?;
        let source_is_active = active_project_id.as_deref() == Some(request.project_id.as_str());
        if source_is_active {
            if request.current_layout.project_id != request.project_id {
                return Err(AppError::InvalidRequest(
                    "Chat rename layout does not belong to the requested Project".into(),
                ));
            }
            validate_tab_projection(&request.current_layout.tabs)?;
            if !request
                .current_layout
                .tabs
                .iter()
                .any(|tab| tab.id == request.chat_id && tab.kind == "chat")
            {
                return Err(AppError::InvalidRequest(
                    "Chat rename target is not attached to the current layout".into(),
                ));
            }
            persist_layout_in(&transaction, &request.current_layout)?;
        }
        apply_chat_title(&transaction, request, &title)?;
        transaction.commit().db()?;
        Ok(())
    }
}
