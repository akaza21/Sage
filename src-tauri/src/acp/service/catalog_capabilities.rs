//! Republishes the capabilities a live session derives from its command
//! catalog, such as Claude Code's goal support through its built-in `/goal`.
//! The negotiated snapshot is stored only when that set changes.

use std::sync::Arc;

use sage_acp_protocol::{intent::CapabilitySnapshotV1, scheduler::RuntimeOwner};
use sage_db::database::Database;

use crate::acp::runtime::{ProjectionPublisher, intersect_capabilities};

pub(crate) struct CatalogCapabilities {
    owner: RuntimeOwner,
    curated: CapabilitySnapshotV1,
    policy: CapabilitySnapshotV1,
    database: Database,
    publisher: Arc<dyn ProjectionPublisher>,
    /// The catalog revision last read.
    revision: Option<u64>,
    /// What the harness last advertised; its first report is the one the
    /// activation already negotiated and stored.
    advertised: Option<CapabilitySnapshotV1>,
    /// The one change not stored yet. A later change replaces it.
    pending: Option<CapabilitySnapshotV1>,
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
            advertised: None,
            pending: None,
        }
    }

    /// Nothing to read while the catalog revision is unchanged and no change waits.
    pub(crate) fn settled(&self, revision: Option<u64>) -> bool {
        self.advertised.is_some() && self.pending.is_none() && self.revision == revision
    }

    /// Takes what the harness advertises at catalog `revision`, storing it
    /// when it differs from what it advertised before.
    pub(crate) fn observe(
        &mut self,
        revision: Option<u64>,
        advertised: Option<CapabilitySnapshotV1>,
    ) {
        self.revision = revision;
        let Some(advertised) = advertised else {
            return;
        };
        match &self.advertised {
            None => self.advertised = Some(advertised),
            Some(last) if *last == advertised => {}
            Some(_) => {
                self.advertised = Some(advertised.clone());
                self.pending = Some(advertised);
            }
        }
        self.store();
    }

    /// A write that fails, or meets a session not ready yet, keeps the change
    /// for the next frame; one outside the binding's ceiling drops it.
    fn store(&mut self) {
        let Some(advertised) = &self.pending else {
            return;
        };
        let Ok(negotiated) = intersect_capabilities(&self.curated, advertised, &self.policy) else {
            self.pending = None;
            return;
        };
        let Ok(mut connection) = self.database.write_connection() else {
            return;
        };
        let stored = crate::acp::store::catalog_capabilities::refresh_catalog_capabilities(
            &mut connection,
            &self.owner,
            &negotiated,
            sage_core::time::now_millis(),
        );
        drop(connection);
        match stored {
            Ok(Some(projection_revision)) => {
                self.pending = None;
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
            Ok(None) => self.pending = None,
            Err(_) => {}
        }
    }
}
