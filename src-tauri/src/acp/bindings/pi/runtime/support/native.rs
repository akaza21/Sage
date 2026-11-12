//! Native RPCs on Pi's session: requests outside any Turn whose answers go
//! back to the caller that asked, such as the usage popover's session stats.

use serde_json::Value;

use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
        pi::{NativeCommand, NativeRequest, Operation},
    },
    goal::NativeRpcReply,
    session_control::SessionControl,
    usage::{SessionSpend, UsageSnapshot},
};

use super::super::{PiRuntime, RequestKind};

impl PiRuntime {
    /// Sends one native request; Pi's session must be open and no other
    /// native request may be waiting on its answer.
    pub(in crate::acp::bindings::pi::runtime) fn begin_native(
        &mut self,
        request: NativeRequest,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.poisoned || self.external_session.is_none() || self.native_pending.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        self.native_rpc_reply = None;
        let egress = self.begin_one(Operation::Native(request), RequestKind::Native)?;
        self.native_pending = self
            .reservations
            .get(
                &egress
                    .token()
                    .request_id()
                    .ok_or(AdapterRuntimeError::Operation)?,
            )
            .map(|reservation| reservation.command_id.clone());
        Ok(egress)
    }

    /// The request for a session control Pi can carry out.
    pub(in crate::acp::bindings::pi::runtime) fn begin_control(
        &mut self,
        control: &SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        match control {
            SessionControl::SetTitle { title } => self.begin_native(NativeRequest::SessionName {
                name: title.as_str().to_owned(),
            }),
            SessionControl::UndoTurn { .. }
            | SessionControl::StopActivity { .. }
            | SessionControl::ReadMcpServers
            | SessionControl::ReloadMcpServers
            | SessionControl::SignInMcpServer { .. } => {
                Err(AdapterRuntimeError::UnsupportedCommand)
            }
        }
    }

    /// Pi's answer to a native request. A refusal or an answer Sage cannot
    /// read is `Rejected`. Stats no caller waits for any more still refresh
    /// the usage snapshot, but answer no one.
    pub(super) fn native_reply(
        &mut self,
        command_id: &str,
        command: NativeCommand,
        data: Option<Value>,
    ) -> AdapterEvent {
        let awaited = self
            .native_pending
            .take_if(|pending| pending == command_id)
            .is_some();
        let reply = match (command, data) {
            (NativeCommand::SessionStats, Some(data)) => match session_spend(&data) {
                Some(session) => {
                    self.usage.replace(UsageSnapshot {
                        session: Some(session),
                        ..UsageSnapshot::default()
                    });
                    NativeRpcReply::UsageRead
                }
                None => NativeRpcReply::Rejected,
            },
            (NativeCommand::SessionName, Some(_)) => NativeRpcReply::Controlled(None),
            (_, None) => NativeRpcReply::Rejected,
        };
        if !awaited {
            return AdapterEvent::IgnoredLate;
        }
        self.native_rpc_reply = Some(reply);
        AdapterEvent::Absorbed
    }

    /// The caller gave up waiting for the native answer; Pi's late reply
    /// then answers no one.
    pub(in crate::acp::bindings::pi::runtime) fn abandon_native(&mut self) {
        self.native_pending = None;
    }
}

/// The session totals of Pi's `get_session_stats`. `tokens` and `cost`
/// cover every assistant reply, tool and summary of the whole session.
fn session_spend(data: &Value) -> Option<SessionSpend> {
    let tokens = data.get("tokens")?;
    let count = |field: &str| tokens.get(field).and_then(Value::as_u64);
    Some(
        SessionSpend {
            input_tokens: count("input")?,
            output_tokens: count("output")?,
            cache_read_tokens: count("cacheRead")?,
            cache_write_tokens: count("cacheWrite")?,
            total_tokens: count("total")?,
            cost_usd: None,
        }
        .with_cost(data.get("cost").and_then(Value::as_f64)),
    )
}

#[cfg(test)]
mod tests {
    use super::session_spend;
    use serde_json::json;

    #[test]
    fn session_stats_read_pis_totals_and_drop_an_unusable_cost() {
        let stats = json!({
            "sessionId":"abc", "userMessages":5,
            "tokens":{"input":50000,"output":10000,"cacheRead":40000,"cacheWrite":5000,"total":105000},
            "cost":0.45,
            "contextUsage":{"tokens":60000,"contextWindow":200000,"percent":30}
        });
        let spend = session_spend(&stats).unwrap();
        assert_eq!(
            (
                spend.input_tokens,
                spend.output_tokens,
                spend.cache_read_tokens,
                spend.cache_write_tokens,
                spend.total_tokens,
                spend.cost_usd
            ),
            (50_000, 10_000, 40_000, 5_000, 105_000, Some(0.45))
        );
        for cost in [json!(-1), json!("0.45"), json!(null)] {
            let mut stats = stats.clone();
            stats["cost"] = cost;
            assert_eq!(session_spend(&stats).unwrap().cost_usd, None);
        }
        let mut partial = stats.clone();
        partial["tokens"]["cacheWrite"] = json!(-5);
        assert!(session_spend(&partial).is_none());
        assert!(session_spend(&json!({"cost":1})).is_none());
    }
}
