use super::*;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(super) async fn commit_and_egress(
        &mut self,
        durable: Vec<DurableAdapterEvent>,
        actual_bytes: usize,
        egress: Vec<AdapterEgress>,
    ) -> Result<(), AdapterPumpError> {
        let mut new_permission = None;
        for event in &durable {
            if let DurableAdapterEvent::PermissionRequest { intent, .. } = event {
                if new_permission.is_some() || self.pending_permission.is_some() {
                    self.teardown = true;
                    return Err(AdapterPumpError::TeardownRequired);
                }
                new_permission = Some(intent.clone());
            }
        }
        let terminal = durable.iter().rev().find_map(|event| match event {
            DurableAdapterEvent::Turn { state, .. } if state.is_terminal() => Some(*state),
            _ => None,
        });
        let committed = if durable.is_empty() {
            None
        } else {
            match self.journal.commit(durable, actual_bytes).await {
                Ok(receipt) => Some(receipt),
                Err(PumpJournalError::CommitUnknown | PumpJournalError::Rejected) => {
                    self.pending.clear();
                    self.teardown = true;
                    return Err(AdapterPumpError::TeardownRequired);
                }
            }
        };
        if let Some(receipt) = committed.as_ref()
            && self.accept_receipt(receipt, terminal).is_err()
        {
            self.pending.clear();
            self.teardown = true;
            return Err(AdapterPumpError::TeardownRequired);
        }

        if let Some(intent) = new_permission.take() {
            self.pending_permission = Some(PendingPermission {
                intent,
                written: false,
                decision: None,
            });
        }
        let mut egress_error = None;
        for value in egress {
            if value.token().is_mandatory()
                && self.adapter.commit_mandatory_egress(value.token()).is_err()
            {
                self.teardown = true;
                egress_error = Some(AdapterPumpError::TeardownRequired);
                break;
            }
            if let Err(error) = self.enqueue(value) {
                egress_error = Some(error);
                break;
            }
        }
        if let Some(receipt) = committed {
            self.publish(receipt)?;
        }
        if let Some(error) = egress_error {
            return Err(error);
        }
        Ok(())
    }

    fn publish(&mut self, receipt: JournalReceipt) -> Result<(), AdapterPumpError> {
        self.publish_revision(receipt.projection_revision)
    }

    pub(super) fn publish_revision(
        &mut self,
        projection_revision: ProjectionRevision,
    ) -> Result<(), AdapterPumpError> {
        let disposition = self
            .publisher
            .publish_committed(crate::acp::intent::ProjectionChangedV1 {
                project_id: self.intent.project_id.clone(),
                chat_id: self.intent.chat_id.clone(),
                session_id: self.intent.session_id.clone(),
                projection_revision,
            })
            .map_err(|_| {
                self.projection_recovery_required = true;
                AdapterPumpError::RecoveryRequired
            })?;
        match disposition {
            ProjectionPublishStatus::BootstrapRequired => {
                self.projection_recovery_required = true;
                Err(AdapterPumpError::BootstrapRequired)
            }
            ProjectionPublishStatus::RecoveryRequired => {
                self.projection_recovery_required = true;
                Err(AdapterPumpError::RecoveryRequired)
            }
            ProjectionPublishStatus::Published | ProjectionPublishStatus::Coalesced => Ok(()),
        }
    }
}
