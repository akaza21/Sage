mod lifecycle;
mod main_window;

pub(crate) use lifecycle::RendererRecreationReloadLifecycle;
pub(crate) use main_window::MainWindowRendererReload;

use crate::acp::{projection::AcpChatPage, store::StoreError};
use sage_acp_protocol::contract::{SageChatId, SageProjectId};

use super::{
    AcpService,
    renderer_recreation::RendererRecreationNativeSnapshot,
    renderer_recreation_packet::{
        RendererRecreationPacketAdmitError, admit_renderer_recreation_frontend_packet,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RendererRecreationPairError {
    NativeInvariant,
    PageIdentity,
    StaleRevision,
    Frontend(RendererRecreationPacketAdmitError),
    AnnouncementPair,
    ProjectionSlots,
    #[cfg(test)]
    Reload,
    Snapshot,
    FrontendRead,
}

impl From<StoreError> for RendererRecreationPairError {
    fn from(_: StoreError) -> Self {
        Self::Snapshot
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RendererReloadError {
    Failed,
}

#[cfg(test)]
pub(crate) trait RendererReload {
    fn reload_main(&self) -> Result<(), RendererReloadError>;
}

pub(crate) trait RendererRecreationAfterPage {
    fn after_page(
        &self,
        service: &AcpService,
        project_id: SageProjectId,
        chat_id: SageChatId,
    ) -> Result<AcpChatPage, StoreError>;
}

pub(crate) struct LoadChatPageAfterEvidence;

impl RendererRecreationAfterPage for LoadChatPageAfterEvidence {
    fn after_page(
        &self,
        service: &AcpService,
        project_id: SageProjectId,
        chat_id: SageChatId,
    ) -> Result<AcpChatPage, StoreError> {
        service.load_chat_page(project_id, chat_id, None)
    }
}

pub(crate) trait RendererRecreationFrontendPacket {
    fn after_frontend_packet(&self) -> Result<String, RendererRecreationPairError>;
}

pub(crate) fn pair_renderer_recreation(
    before: &RendererRecreationNativeSnapshot,
    after: &RendererRecreationNativeSnapshot,
    before_revision: i64,
    after_page: (&SageProjectId, &SageChatId, i64),
    after_frontend: &str,
) -> Result<(), RendererRecreationPairError> {
    if before != after {
        return Err(RendererRecreationPairError::NativeInvariant);
    }
    if after_page.0 != before.project_id() || after_page.1 != before.chat_id() {
        return Err(RendererRecreationPairError::PageIdentity);
    }
    if after_page.2 < before_revision {
        return Err(RendererRecreationPairError::StaleRevision);
    }
    if after_page.2 != after.chat_projection_revision() {
        return Err(RendererRecreationPairError::StaleRevision);
    }
    let packet = admit_renderer_recreation_frontend_packet(after_frontend)
        .map_err(RendererRecreationPairError::Frontend)?;
    if !packet.is_announcement_pair() {
        return Err(RendererRecreationPairError::AnnouncementPair);
    }
    if packet.projection_slots() < 1 || packet.projection_slots() > 2 {
        return Err(RendererRecreationPairError::ProjectionSlots);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn pair_renderer_recreation_after_reload<R, P, F>(
    service: &AcpService,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    reload: &R,
    after_page: &P,
    frontend: &F,
) -> Result<(), RendererRecreationPairError>
where
    R: RendererReload,
    P: RendererRecreationAfterPage,
    F: RendererRecreationFrontendPacket,
{
    let before = service.renderer_recreation_native_snapshot(project_id, chat_id)?;
    let before_revision = before.chat_projection_revision();
    reload
        .reload_main()
        .map_err(|_| RendererRecreationPairError::Reload)?;
    let page = after_page.after_page(service, project_id.clone(), chat_id.clone())?;
    let after_revision = i64::try_from(page.projection_revision.get())
        .map_err(|_| RendererRecreationPairError::Snapshot)?;
    let after_frontend = frontend.after_frontend_packet()?;
    let after = service.renderer_recreation_native_snapshot(project_id, chat_id)?;
    pair_renderer_recreation(
        &before,
        &after,
        before_revision,
        (&page.project_id, &page.chat_id, after_revision),
        &after_frontend,
    )
}

pub(crate) fn pair_renderer_recreation_after_observed_reload<F>(
    service: &AcpService,
    before: RendererRecreationNativeSnapshot,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    frontend: &F,
) -> Result<(), RendererRecreationPairError>
where
    F: RendererRecreationFrontendPacket,
{
    let page =
        LoadChatPageAfterEvidence.after_page(service, project_id.clone(), chat_id.clone())?;
    let after_revision = i64::try_from(page.projection_revision.get())
        .map_err(|_| RendererRecreationPairError::Snapshot)?;
    let after_frontend = frontend.after_frontend_packet()?;
    let after = service.renderer_recreation_native_snapshot(project_id, chat_id)?;
    pair_renderer_recreation(
        &before,
        &after,
        before.chat_projection_revision(),
        (&page.project_id, &page.chat_id, after_revision),
        &after_frontend,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_db::database::Database;

    fn announcement_packet(slots: u8) -> String {
        format!(
            "{{\"schemaVersion\":1,\"liveRegions\":{{\"polite\":1,\"assertive\":1}},\"pagehideListeners\":1,\"focusOutcome\":\"exact-sage-id\",\"projectionSlots\":{slots}}}"
        )
    }

    fn seeded_reload_chat() -> (AcpService, Database, SageProjectId, SageChatId) {
        let database = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
        database
            .connection()
            .execute_batch(
                "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
                 VALUES('project-reload','Reload',1,1,1);
                 INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal)
                 VALUES('root-reload','project-reload','C:/reload','c:/reload',0);
                 INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES('chat-reload','project-reload',0,'Reload',7,1,1);",
            )
            .unwrap();
        let service = AcpService::new(database.clone()).unwrap();
        (
            service,
            database,
            SageProjectId::parse("project-reload").unwrap(),
            SageChatId::parse("chat-reload").unwrap(),
        )
    }

    fn capture(
        service: &AcpService,
        project_id: &SageProjectId,
        chat_id: &SageChatId,
    ) -> RendererRecreationNativeSnapshot {
        service
            .renderer_recreation_native_snapshot(project_id, chat_id)
            .unwrap()
    }

    fn pair(
        before: &RendererRecreationNativeSnapshot,
        after: &RendererRecreationNativeSnapshot,
        before_revision: i64,
        after_page: (&SageProjectId, &SageChatId, i64),
        frontend: &str,
    ) -> Result<(), RendererRecreationPairError> {
        pair_renderer_recreation(before, after, before_revision, after_page, frontend)
    }

    #[test]
    fn pairs_identical_native_snapshots_at_equal_revision_with_one_projection_slot() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        let after = capture(&service, &project_id, &chat_id);
        let revision = before.chat_projection_revision();

        pair(
            &before,
            &after,
            revision,
            (&project_id, &chat_id, revision),
            &announcement_packet(1),
        )
        .unwrap();
    }

    #[test]
    fn pairs_identical_native_snapshots_at_equal_revision_with_two_projection_slots() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        let after = capture(&service, &project_id, &chat_id);
        let revision = before.chat_projection_revision();

        pair(
            &before,
            &after,
            revision,
            (&project_id, &chat_id, revision),
            &announcement_packet(2),
        )
        .unwrap();
    }

    #[test]
    fn rejects_cloned_identical_snapshots_when_after_page_revision_is_newer() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        let after = before.clone();
        let revision = before.chat_projection_revision();

        assert_eq!(
            pair(
                &before,
                &after,
                revision,
                (&project_id, &chat_id, revision + 1),
                &announcement_packet(2),
            ),
            Err(RendererRecreationPairError::StaleRevision)
        );
    }

    #[test]
    fn rejects_mutated_sqlite_title_as_native_drift() {
        let (service, database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        database
            .connection()
            .execute(
                "UPDATE chats SET title='Changed' WHERE id='chat-reload'",
                [],
            )
            .unwrap();
        let after = capture(&service, &project_id, &chat_id);
        let revision = before.chat_projection_revision();

        assert_eq!(
            pair(
                &before,
                &after,
                revision,
                (&project_id, &chat_id, revision),
                &announcement_packet(1),
            ),
            Err(RendererRecreationPairError::NativeInvariant)
        );
    }

    #[test]
    fn rejects_after_page_for_a_different_chat() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        let after = before.clone();
        let revision = before.chat_projection_revision();
        let other_chat = SageChatId::parse("chat-other").unwrap();

        assert_eq!(
            pair(
                &before,
                &after,
                revision,
                (&project_id, &other_chat, revision),
                &announcement_packet(1),
            ),
            Err(RendererRecreationPairError::PageIdentity)
        );
    }

    #[test]
    fn rejects_an_older_after_page_revision() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        let after = before.clone();
        let revision = before.chat_projection_revision();

        assert_eq!(
            pair(
                &before,
                &after,
                revision,
                (&project_id, &chat_id, revision - 1),
                &announcement_packet(1),
            ),
            Err(RendererRecreationPairError::StaleRevision)
        );
    }

    #[test]
    fn rejects_frontend_packets_that_fail_s1_admit() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        let after = before.clone();
        let revision = before.chat_projection_revision();
        let pagehide_two = "{\"schemaVersion\":1,\"liveRegions\":{\"polite\":1,\"assertive\":1},\"pagehideListeners\":2,\"focusOutcome\":\"exact-sage-id\",\"projectionSlots\":1}";
        let four_live_regions = "{\"schemaVersion\":1,\"liveRegions\":{\"polite\":2,\"assertive\":2},\"pagehideListeners\":1,\"focusOutcome\":\"exact-sage-id\",\"projectionSlots\":1}";

        assert_eq!(
            pair(
                &before,
                &after,
                revision,
                (&project_id, &chat_id, revision),
                pagehide_two,
            ),
            Err(RendererRecreationPairError::Frontend(
                RendererRecreationPacketAdmitError::InvalidPagehide
            ))
        );
        assert_eq!(
            pair(
                &before,
                &after,
                revision,
                (&project_id, &chat_id, revision),
                four_live_regions,
            ),
            Err(RendererRecreationPairError::Frontend(
                RendererRecreationPacketAdmitError::InvalidLiveRegions
            ))
        );
    }

    #[test]
    fn rejects_empty_or_zero_slot_packets_after_reload() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        let after = before.clone();
        let revision = before.chat_projection_revision();
        let empty = "{\"schemaVersion\":1,\"liveRegions\":{\"polite\":0,\"assertive\":0},\"pagehideListeners\":0,\"focusOutcome\":\"exact-sage-id\",\"projectionSlots\":0}";

        assert_eq!(
            pair(
                &before,
                &after,
                revision,
                (&project_id, &chat_id, revision),
                empty,
            ),
            Err(RendererRecreationPairError::AnnouncementPair)
        );
        assert_eq!(
            pair(
                &before,
                &after,
                revision,
                (&project_id, &chat_id, revision),
                &announcement_packet(0),
            ),
            Err(RendererRecreationPairError::ProjectionSlots)
        );
    }

    struct RecordingReload {
        calls: std::rc::Rc<std::cell::Cell<u8>>,
        fails: bool,
    }

    impl RecordingReload {
        fn succeeding() -> Self {
            Self {
                calls: std::rc::Rc::new(std::cell::Cell::new(0)),
                fails: false,
            }
        }

        fn failing() -> Self {
            Self {
                calls: std::rc::Rc::new(std::cell::Cell::new(0)),
                fails: true,
            }
        }
    }

    impl RendererReload for RecordingReload {
        fn reload_main(&self) -> Result<(), RendererReloadError> {
            self.calls.set(self.calls.get().saturating_add(1));
            if self.fails {
                Err(RendererReloadError::Failed)
            } else {
                Ok(())
            }
        }
    }

    struct RecordingAfterPage {
        reload_calls: std::rc::Rc<std::cell::Cell<u8>>,
        calls: std::cell::Cell<u8>,
        foreign_chat: Option<SageChatId>,
    }

    impl RecordingAfterPage {
        fn from_reload(reload: &RecordingReload, foreign_chat: Option<SageChatId>) -> Self {
            Self {
                reload_calls: reload.calls.clone(),
                calls: std::cell::Cell::new(0),
                foreign_chat,
            }
        }
    }

    impl RendererRecreationAfterPage for RecordingAfterPage {
        fn after_page(
            &self,
            service: &AcpService,
            project_id: SageProjectId,
            chat_id: SageChatId,
        ) -> Result<AcpChatPage, StoreError> {
            if self.reload_calls.get() == 0 {
                panic!("after-page consulted before reload_main");
            }
            self.calls.set(self.calls.get().saturating_add(1));
            let chat_id = self.foreign_chat.clone().unwrap_or(chat_id);
            service.load_chat_page(project_id, chat_id, None)
        }
    }

    struct RecordingFrontendPacket {
        reload_calls: std::rc::Rc<std::cell::Cell<u8>>,
        calls: std::cell::Cell<u8>,
        packet: Result<String, RendererRecreationPairError>,
    }

    impl RecordingFrontendPacket {
        fn announcement(reload: &RecordingReload, slots: u8) -> Self {
            Self {
                reload_calls: reload.calls.clone(),
                calls: std::cell::Cell::new(0),
                packet: Ok(announcement_packet(slots)),
            }
        }

        fn raw(reload: &RecordingReload, packet: &str) -> Self {
            Self {
                reload_calls: reload.calls.clone(),
                calls: std::cell::Cell::new(0),
                packet: Ok(packet.to_string()),
            }
        }

        fn failing(reload: &RecordingReload, error: RendererRecreationPairError) -> Self {
            Self {
                reload_calls: reload.calls.clone(),
                calls: std::cell::Cell::new(0),
                packet: Err(error),
            }
        }
    }

    impl RendererRecreationFrontendPacket for RecordingFrontendPacket {
        fn after_frontend_packet(&self) -> Result<String, RendererRecreationPairError> {
            if self.reload_calls.get() == 0 {
                panic!("frontend packet consulted before reload_main");
            }
            self.calls.set(self.calls.get().saturating_add(1));
            self.packet.clone()
        }
    }

    #[test]
    fn successful_reload_after_page_comes_from_load_chat_page() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        assert_eq!(before.chat_projection_revision(), 7);
        let reload = RecordingReload::succeeding();
        let after_page = LoadChatPageAfterEvidence;
        let frontend = RecordingFrontendPacket::announcement(&reload, 1);

        pair_renderer_recreation_after_reload(
            &service,
            &project_id,
            &chat_id,
            &reload,
            &after_page,
            &frontend,
        )
        .unwrap();

        let page = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        assert_eq!(page.project_id, project_id);
        assert_eq!(page.chat_id, chat_id);
        assert_eq!(page.projection_revision.get(), 7);
        assert_eq!(reload.calls.get(), 1);
        assert_eq!(frontend.calls.get(), 1);
        assert_eq!(capture(&service, &project_id, &chat_id), before);
    }

    #[test]
    fn successful_reload_pairs_announcement_slot_two_from_live_page() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let before = capture(&service, &project_id, &chat_id);
        let reload = RecordingReload::succeeding();
        let after_page = RecordingAfterPage::from_reload(&reload, None);
        let frontend = RecordingFrontendPacket::announcement(&reload, 2);

        pair_renderer_recreation_after_reload(
            &service,
            &project_id,
            &chat_id,
            &reload,
            &after_page,
            &frontend,
        )
        .unwrap();
        assert_eq!(reload.calls.get(), 1);
        assert_eq!(after_page.calls.get(), 1);
        assert_eq!(frontend.calls.get(), 1);
        assert_eq!(capture(&service, &project_id, &chat_id), before);
    }

    #[test]
    fn missing_chat_after_reload_fails_pair_without_frontend_identity() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let reload = RecordingReload::succeeding();
        let missing = SageChatId::parse("chat-missing").unwrap();
        let after_page = RecordingAfterPage::from_reload(&reload, Some(missing));
        let frontend = RecordingFrontendPacket::announcement(&reload, 1);

        assert_eq!(
            pair_renderer_recreation_after_reload(
                &service,
                &project_id,
                &chat_id,
                &reload,
                &after_page,
                &frontend,
            ),
            Err(RendererRecreationPairError::Snapshot)
        );
        assert_eq!(reload.calls.get(), 1);
        assert_eq!(after_page.calls.get(), 1);
        assert_eq!(frontend.calls.get(), 0);
    }

    #[test]
    fn foreign_chat_after_reload_fails_pair_without_using_frontend_as_page() {
        let (service, database, project_id, chat_id) = seeded_reload_chat();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES('chat-foreign','project-reload',1,'Foreign',7,1,1)",
                [],
            )
            .unwrap();
        let foreign = SageChatId::parse("chat-foreign").unwrap();
        let reload = RecordingReload::succeeding();
        let after_page = RecordingAfterPage::from_reload(&reload, Some(foreign));
        let frontend = RecordingFrontendPacket::announcement(&reload, 2);

        assert_eq!(
            pair_renderer_recreation_after_reload(
                &service,
                &project_id,
                &chat_id,
                &reload,
                &after_page,
                &frontend,
            ),
            Err(RendererRecreationPairError::PageIdentity)
        );
        assert_eq!(reload.calls.get(), 1);
        assert_eq!(after_page.calls.get(), 1);
    }

    #[test]
    fn reload_failure_never_reads_page_or_frontend_evidence() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let reload = RecordingReload::failing();
        let after_page = RecordingAfterPage::from_reload(&reload, None);
        let frontend = RecordingFrontendPacket::announcement(&reload, 1);

        assert_eq!(
            pair_renderer_recreation_after_reload(
                &service,
                &project_id,
                &chat_id,
                &reload,
                &after_page,
                &frontend,
            ),
            Err(RendererRecreationPairError::Reload)
        );
        assert_eq!(reload.calls.get(), 1);
        assert_eq!(after_page.calls.get(), 0);
        assert_eq!(frontend.calls.get(), 0);
    }

    #[test]
    fn empty_or_invalid_frontend_packet_after_reload_fails_through_admit() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let reload = RecordingReload::succeeding();
        let after_page = RecordingAfterPage::from_reload(&reload, None);
        let empty = RecordingFrontendPacket::raw(&reload, "");
        let invalid = RecordingFrontendPacket::raw(&reload, "{");

        assert_eq!(
            pair_renderer_recreation_after_reload(
                &service,
                &project_id,
                &chat_id,
                &reload,
                &after_page,
                &empty,
            ),
            Err(RendererRecreationPairError::Frontend(
                RendererRecreationPacketAdmitError::InvalidJson
            ))
        );
        assert_eq!(
            pair_renderer_recreation_after_reload(
                &service,
                &project_id,
                &chat_id,
                &reload,
                &after_page,
                &invalid,
            ),
            Err(RendererRecreationPairError::Frontend(
                RendererRecreationPacketAdmitError::InvalidJson
            ))
        );
        assert_eq!(reload.calls.get(), 2);
        assert_eq!(after_page.calls.get(), 2);
        assert_eq!(empty.calls.get(), 1);
        assert_eq!(invalid.calls.get(), 1);
    }

    #[test]
    fn frontend_packet_port_error_after_reload_fails_pair() {
        let (service, _database, project_id, chat_id) = seeded_reload_chat();
        let reload = RecordingReload::succeeding();
        let after_page = RecordingAfterPage::from_reload(&reload, None);
        let frontend = RecordingFrontendPacket::failing(
            &reload,
            RendererRecreationPairError::Frontend(RendererRecreationPacketAdmitError::InvalidJson),
        );

        assert_eq!(
            pair_renderer_recreation_after_reload(
                &service,
                &project_id,
                &chat_id,
                &reload,
                &after_page,
                &frontend,
            ),
            Err(RendererRecreationPairError::Frontend(
                RendererRecreationPacketAdmitError::InvalidJson
            ))
        );
        assert_eq!(reload.calls.get(), 1);
        assert_eq!(after_page.calls.get(), 1);
        assert_eq!(frontend.calls.get(), 1);
    }
}
