//! Session configuration state above the protocol types: the live channel
//! each runtime publishes its configuration through, and the confirmed
//! choices and notices stored in SQLite. The configuration values themselves
//! are `sage_acp_protocol::session_config`.

mod channel;
pub(crate) mod choices;
pub(crate) mod notices;
pub(crate) use channel::{ChoiceRecorder, ConfigChannel, NoticeSink, SnapshotRecorder};
