use sage_acp_protocol::intent::session_event::SessionScopedEvent;

use super::{CodexRuntime, support::one};
use crate::acp::{
    bindings::{
        adapter::{AdapterBatch, AdapterEvent, AdapterRuntimeError},
        codex::incoming::{IncomingEvent, IngressDisposition, rate_limits::RateLimitBucket},
    },
    goal::NativeRpcReply,
};

impl CodexRuntime {
    pub(super) fn apply_usage(&mut self, disposition: IngressDisposition) {
        match disposition {
            IngressDisposition::UsageUpdated(bucket) => {
                self.rate_limits.push(bucket);
                self.usage.replace(self.rate_limits.snapshot());
            }
            IngressDisposition::UsageRead(Some(limits)) => {
                self.usage.replace(limits.snapshot());
                self.rate_limits = limits;
                self.native_rpc_reply = Some(NativeRpcReply::UsageRead);
            }
            IngressDisposition::UsageRead(None) => {
                self.native_rpc_reply = Some(NativeRpcReply::Rejected);
            }
            IngressDisposition::UsageRefused => {
                self.usage.mark_unsupported();
                self.native_rpc_reply = Some(NativeRpcReply::Rejected);
            }
            IngressDisposition::UsageReset => self.usage.reset(),
            _ => {}
        }
    }

    /// The app-server serves one bound thread, so the account's limits show
    /// in its session; before one binds, the next push reports them.
    pub(super) fn usage_pushed(
        &mut self,
        bucket: RateLimitBucket,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let notices = bucket.limit_notices();
        self.apply_usage(IngressDisposition::UsageUpdated(bucket));
        let session = self
            .inner
            .bound_thread_id()
            .and_then(|thread| self.inner.thread_session_id(thread).ok());
        let Some(id) = session.filter(|_| !notices.is_empty()) else {
            return one(AdapterEvent::IgnoredLate);
        };
        let events = notices
            .into_iter()
            .map(|notice| IncomingEvent::Session {
                id: id.clone(),
                event: SessionScopedEvent::Notice(notice),
            })
            .collect();
        self.translate(IngressDisposition::Events(events), request_digest)
    }
}
