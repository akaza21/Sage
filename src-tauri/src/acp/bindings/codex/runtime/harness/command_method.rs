use crate::acp::bindings::adapter::AdapterCommand;

pub(super) fn command_method(command: &AdapterCommand) -> &'static str {
    match command {
        AdapterCommand::Initialize => "initialize",
        AdapterCommand::ModelPage { .. } => "model/list",
        AdapterCommand::NewSession { .. } => "thread/start",
        AdapterCommand::ResumeSession { .. } => "thread/resume",
        AdapterCommand::LoadSession { .. } => "thread/read",
        AdapterCommand::CloseSession => "thread/archive",
        AdapterCommand::Prompt { .. } => "turn/start",
        AdapterCommand::Steer { .. } => "turn/steer",
        AdapterCommand::Cancel => "turn/interrupt",
        AdapterCommand::ResolvePermission { .. } => "requestPermission",
    }
}
