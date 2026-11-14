use super::CodexRuntime;
use crate::acp::{bindings::codex::incoming::IngressDisposition, goal::NativeRpcReply};

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
}
