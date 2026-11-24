use super::super::PiRuntime;
use crate::acp::bindings::{
    adapter::{AdapterBatch, AdapterRuntimeError},
    pi::bridge::install,
};
use serde::Deserialize;

/// A bridge status body past this is not read.
const STATUS_BYTES: usize = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    version: u8,
    digest: String,
}

/// Pi's `model_select`: any change of model, Sage's own or an extension's.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelSelected {
    version: u8,
    provider: String,
    id: String,
}

/// Pi's `input` from `pi.sendUserMessage`: the SHA-256 of the text the
/// extension sent, one marker per message.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserMessage {
    version: u8,
    digest: String,
}

fn body<'a, T: Deserialize<'a>>(value: &'a serde_json::Value) -> Option<T> {
    value
        .get("statusText")
        .and_then(serde_json::Value::as_str)
        .filter(|body| body.len() <= STATUS_BYTES)
        .and_then(|body| serde_json::from_str(body).ok())
}

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn require_bridge_ready(
        &mut self,
    ) -> Result<(), AdapterRuntimeError> {
        if self.bridge.as_ref().is_some_and(|bridge| !bridge.ready) {
            self.poisoned = true;
            return Err(AdapterRuntimeError::ApprovalBridgeUnavailable);
        }
        Ok(())
    }

    /// The bridge's `sage.pi.*` statuses. Unknown keys and malformed model or
    /// user markers are dropped: they only ever add to what Sage shows.
    pub(in crate::acp::bindings::pi::runtime) fn bridge_status(
        &mut self,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        match key {
            "sage.pi.ready.v1" => return self.bridge_ready(value),
            "sage.pi.model.v1" => {
                if let Some(selected) =
                    body::<ModelSelected>(value).filter(|model| model.version == 1)
                {
                    self.bridge_model(&selected.provider, &selected.id)?;
                }
            }
            "sage.pi.user.v1" => {
                let digest = body::<UserMessage>(value)
                    .filter(|marker| marker.version == 1)
                    .and_then(|marker| decode_digest(&marker.digest));
                if let Some(digest) = digest {
                    self.translator.mark_injected_user_message(digest);
                }
            }
            _ => {}
        }
        AdapterBatch::new(Vec::new(), Vec::new())
    }

    /// The same update as Sage's own `set_model` reply, so the model chip
    /// follows. The current model is a no-op; a model outside the catalogue
    /// is read with the next prompt, like other extension changes. While
    /// Sage's own change is in flight it is kept, to check that change's reply.
    fn bridge_model(&mut self, provider: &str, id: &str) -> Result<(), AdapterRuntimeError> {
        // Until the session opens, Sage's own state read carries the model.
        if !self.initialized || self.external_session.is_none() {
            return Ok(());
        }
        if self.translator.set_model_pending() {
            self.model_hint = Some((provider.to_owned(), id.to_owned()));
        }
        let Some(model) = self
            .available_models
            .iter()
            .find(|model| model.provider == provider && model.id == id)
            .cloned()
        else {
            self.configuration_dirty = true;
            return Ok(());
        };
        if self.current_model.as_deref() != Some(super::identity::model_key(&model)?.as_str()) {
            self.model_selected(model)?;
        }
        Ok(())
    }

    fn bridge_ready(
        &mut self,
        value: &serde_json::Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        // session_shutdown precedes extension reload as well as process exit.
        // A previous runner's handshake cannot admit the replacement runner.
        if value
            .get("statusText")
            .is_none_or(serde_json::Value::is_null)
        {
            if let Some(bridge) = self.bridge.as_mut() {
                bridge.ready = false;
            }
            return AdapterBatch::new(Vec::new(), Vec::new());
        }
        let valid = value
            .get("statusText")
            .and_then(serde_json::Value::as_str)
            .filter(|body| body.len() <= 128)
            .and_then(|body| serde_json::from_str::<Ready>(body).ok())
            .is_some_and(|ready| ready.version == 1 && ready.digest == install::digest());
        if !valid {
            self.poisoned = true;
            return Err(AdapterRuntimeError::ApprovalBridgeUnavailable);
        }
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.ready = true;
        }
        AdapterBatch::new(Vec::new(), Vec::new())
    }
}

fn decode_digest(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut digest = [0; 32];
    for (byte, pair) in digest.iter_mut().zip(hex.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(digest)
}
