use sage_core::limits::LIMITS;
use serde::Deserialize;

/// Raw catalogs may exceed the admitted 512 rows. Together with the existing frame-byte,
/// node and depth limits, this bounds temporary decoding before shared catalog admission.
const RAW_COMMANDS: u64 = 2048;

/// Only the command array in a recognized catalog envelope gets the raw ceiling.
/// Children of each row retain the ordinary structural limits.
#[derive(Clone, Copy, Default)]
pub(crate) enum CommandWirePath {
    /// A binding-declared object field or array element on the catalog path.
    Field(&'static str, &'static Self),
    Elements(&'static Self),
    Acp,
    Rpc,
    Params,
    Update,
    Data,
    Commands,
    #[default]
    Other,
}

impl CommandWirePath {
    pub(crate) fn for_frame(bytes: &[u8]) -> Self {
        // Ignore catalog contents here, without allocating rows. The actual ingress/decoder
        // still validates every key, node and value, including duplicate envelope keys.
        let Ok(envelope) = serde_json::from_slice::<Envelope>(bytes) else {
            return Self::Other;
        };
        if envelope.jsonrpc.as_deref() == Some("2.0")
            && envelope.method.as_deref() == Some("session/update")
            && envelope
                .params
                .and_then(|params| params.update)
                .and_then(|update| update.session_update)
                .as_deref()
                == Some("available_commands_update")
        {
            Self::Acp
        } else if envelope.r#type.as_deref() == Some("response")
            && envelope.command.as_deref() == Some("get_commands")
            && envelope.success == Some(true)
        {
            Self::Rpc
        } else {
            Self::Other
        }
    }

    pub(crate) fn child(self, key: &str) -> Self {
        match (self, key) {
            (Self::Field(field, next), key) if field == key => *next,
            (Self::Acp, "params") => Self::Params,
            (Self::Params, "update") => Self::Update,
            (Self::Update, "availableCommands") | (Self::Data, "commands") => Self::Commands,
            (Self::Rpc, "data") => Self::Data,
            _ => Self::Other,
        }
    }

    pub(crate) fn element(self) -> Self {
        match self {
            Self::Elements(next) => *next,
            _ => Self::Other,
        }
    }

    pub(crate) fn array_limit(self) -> u64 {
        match self {
            Self::Commands => RAW_COMMANDS,
            _ => LIMITS.wire.array_entries,
        }
    }
}

#[derive(Deserialize)]
struct Envelope {
    jsonrpc: Option<String>,
    method: Option<String>,
    params: Option<Params>,
    r#type: Option<String>,
    command: Option<String>,
    success: Option<bool>,
}

#[derive(Deserialize)]
struct Params {
    update: Option<Update>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Update {
    session_update: Option<String>,
}
