//! A new or resumed Claude runtime driven in memory, for service tests that need
//! the real binding's wire behaviour without a child process.
use super::*;
use crate::acp::bindings::runtime::NativeRuntime;
use crate::acp::{
    scheduler::RuntimeOwner,
    session_config::{ConfigChannel, ConfigIntent},
    transport::IncrementalFrameDecoder,
};

pub(crate) struct InMemoryClaude(NativeRuntime);

fn frame(value: &Value) -> Frame {
    let mut bytes = serde_json::to_vec(value).expect("json");
    bytes.push(b'\n');
    IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .expect("valid frame")
        .expect("complete frame")
}

impl InMemoryClaude {
    /// Opens external session `external-a`, new or resumed, which reports
    /// `config_options` and `mode` of `default`/`ask`/`code`/`auto`, with
    /// the session config published to `channel` as a live runtime does.
    pub(crate) fn opened(
        owner: &RuntimeOwner,
        channel: Arc<ConfigChannel>,
        new: bool,
        mode: &str,
        config_options: Value,
    ) -> Self {
        let mut runtime =
            ClaudeRuntime::new(None, [7; 32], owner.runtime_epoch, owner.session_id.clone());
        super::tests::initialize(&mut runtime);
        let mut runtime = NativeRuntime::new(runtime);
        runtime.attach_config(owner.clone(), channel);
        let mut this = Self(runtime);
        let native_root = "C:\\fixture".to_owned();
        let command = if new {
            AdapterCommand::NewSession {
                native_root,
                model: None,
            }
        } else {
            AdapterCommand::ResumeSession {
                external_session_id: ExternalId::new("session", "external-a").unwrap(),
                native_root,
                model: None,
            }
        };
        let request = this.send(command).expect("open");
        this.receive(json!({"jsonrpc":"2.0","id":request["id"],"result":{
            "sessionId":"external-a",
            "modes":{"currentModeId":mode,"availableModes":[
                {"id":"default","name":"Default"},
                {"id":"ask","name":"Ask"},{"id":"code","name":"Code"},{"id":"auto","name":"Auto"}
            ]},
            "configOptions":config_options
        }}))
        .expect("open response");
        this
    }

    fn send(&mut self, command: AdapterCommand) -> Result<Value, AdapterRuntimeError> {
        let egress = self.0.command(command)?;
        self.written(egress)
    }

    fn written(&mut self, egress: AdapterEgress) -> Result<Value, AdapterRuntimeError> {
        let request = serde_json::from_slice(egress.frame().payload()).expect("json request");
        self.0
            .resolve_egress(egress.into_token(), EgressResolution::Written)?;
        Ok(request)
    }

    /// Lets the binding apply one config change and returns the request it
    /// wrote to the harness, if any.
    pub(crate) fn configure(
        &mut self,
        intent: &ConfigIntent,
    ) -> Result<Option<Value>, AdapterRuntimeError> {
        match self.0.configure(intent)? {
            Some(egress) => self.written(egress).map(Some),
            None => Ok(None),
        }
    }

    /// Delivers one frame from the harness to the binding.
    pub(crate) fn receive(&mut self, value: Value) -> Result<(), AdapterRuntimeError> {
        self.0
            .ingest(&frame(&value), &super::tests::Mapper)
            .map(|_| ())
    }
}
