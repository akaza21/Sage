//! Republishes the capabilities a live session derives from its command
//! catalog, such as Claude Code's goal support through its built-in `/goal`.
//! The store is written only when that set changes.

use std::sync::Arc;

use sage_acp_protocol::{intent::CapabilitySnapshotV1, scheduler::RuntimeOwner};
use sage_db::database::Database;

use crate::acp::{
    runtime::ProjectionPublisher,
    store::catalog_capabilities::{CapabilityBounds, CatalogRefresh, refresh_catalog_capabilities},
};

pub(crate) struct CatalogCapabilities {
    owner: RuntimeOwner,
    curated: CapabilitySnapshotV1,
    policy: CapabilitySnapshotV1,
    database: Database,
    publisher: Arc<dyn ProjectionPublisher>,
    /// The catalog revision last read.
    revision: Option<u64>,
    /// What the store holds as far as this runtime knows; the harness's first
    /// report is the set its activation negotiated and stored.
    stored: Option<CapabilitySnapshotV1>,
    /// A change met a session still starting: it is written again once the
    /// session is ready, not on every frame.
    awaiting_ready: bool,
    retry: bool,
}

impl CatalogCapabilities {
    pub(crate) fn new(
        owner: RuntimeOwner,
        (curated, policy): (CapabilitySnapshotV1, CapabilitySnapshotV1),
        database: Database,
        publisher: Arc<dyn ProjectionPublisher>,
    ) -> Self {
        Self {
            owner,
            curated,
            policy,
            database,
            publisher,
            revision: None,
            stored: None,
            awaiting_ready: false,
            retry: false,
        }
    }

    /// Nothing to read until the catalog revision moves or a change that
    /// waited on activation may now be written.
    pub(crate) fn settled(&self, revision: Option<u64>) -> bool {
        self.stored.is_some() && self.revision == revision && !self.retry
    }

    /// The session's activation has committed: a change it held back goes
    /// out with the next frame.
    pub(crate) fn session_ready(&mut self) {
        self.retry = self.awaiting_ready;
    }

    /// Takes what the harness advertises at catalog `revision`, storing it
    /// when it differs from what the store holds.
    pub(crate) fn observe(
        &mut self,
        revision: Option<u64>,
        advertised: Option<CapabilitySnapshotV1>,
    ) {
        self.revision = revision;
        self.retry = false;
        let Some(advertised) = advertised else {
            return;
        };
        match &self.stored {
            None => self.stored = Some(advertised),
            Some(stored) if *stored == advertised => self.awaiting_ready = false,
            Some(_) => self.store(advertised),
        }
    }

    /// Only a session still starting keeps the change; any other refusal or
    /// failure drops it until the catalog moves again.
    fn store(&mut self, advertised: CapabilitySnapshotV1) {
        self.awaiting_ready = false;
        let Ok(mut connection) = self.database.write_connection() else {
            return;
        };
        let bounds = CapabilityBounds {
            curated: &self.curated,
            policy: &self.policy,
        };
        let result = refresh_catalog_capabilities(
            &mut connection,
            &self.owner,
            bounds,
            &advertised,
            sage_core::time::now_millis(),
        );
        drop(connection);
        match result {
            Ok(CatalogRefresh::Applied(projection_revision)) => {
                self.stored = Some(advertised);
                let _ = self
                    .publisher
                    .publish_committed(crate::acp::ipc::ProjectionChangedV1 {
                        project_id: self.owner.project_id.clone(),
                        chat_id: self.owner.chat_id.clone(),
                        session_id: self.owner.session_id.clone(),
                        projection_revision,
                        child_projection_epoch: 0,
                    });
            }
            Ok(CatalogRefresh::Unchanged) => self.stored = Some(advertised),
            Ok(CatalogRefresh::NotReady) => self.awaiting_ready = true,
            Err(_) => {}
        }
    }
}

#[cfg(all(test, feature = "acp-fixture"))]
mod tests {
    use super::*;

    #[test]
    fn a_change_refused_while_starting_is_kept_until_a_ready_store_takes_it() {
        let (database, owner, negotiated, complete) = crate::acp::store::activating_fixture();
        let session = owner.session_id.as_str().to_owned();
        let stored = || -> String {
            database
                .connection()
                .query_row(
                    "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
                    [session.as_str()],
                    |row| row.get(0),
                )
                .unwrap()
        };
        let mut advertised = negotiated.clone();
        advertised.goal.set = !negotiated.goal.set;
        let notices = Arc::new(crate::acp::runtime::ProjectionMailbox::default());
        let mut capabilities = CatalogCapabilities::new(
            owner,
            (advertised.clone(), advertised.clone()),
            database.clone(),
            notices.clone(),
        );
        capabilities.observe(Some(1), Some(negotiated));
        let before = stored();
        capabilities.observe(Some(2), Some(advertised.clone()));
        // Held back, it is not retried on every frame.
        assert!(capabilities.settled(Some(2)));
        // Readied before the activation commits: refused again, still kept.
        capabilities.session_ready();
        assert!(!capabilities.settled(Some(2)));
        capabilities.observe(Some(2), Some(advertised.clone()));
        assert!(capabilities.settled(Some(2)));
        assert_eq!(stored(), before);
        assert!(notices.take().is_none());
        complete(&mut database.write_connection().unwrap());
        capabilities.session_ready();
        capabilities.observe(Some(2), Some(advertised.clone()));
        let after: CapabilitySnapshotV1 = serde_json::from_str(&stored()).unwrap();
        assert_eq!(after.goal, advertised.goal);
        assert!(notices.take().is_some());
        assert!(capabilities.settled(Some(2)));
    }
}
