use crate::acp::contract::ContractError;

#[path = "command.rs"]
pub(crate) mod command;
#[path = "credentials.rs"]
pub(crate) mod credentials;
#[path = "embedded.rs"]
mod embedded;
#[path = "paths.rs"]
pub(crate) mod paths;
#[path = "pem.rs"]
pub(crate) mod pem;
#[path = "prose.rs"]
mod prose;
#[path = "scalars.rs"]
mod scalars;
#[path = "secrets.rs"]
pub(crate) mod secrets;
#[path = "shell.rs"]
pub(crate) mod shell;
#[path = "spans.rs"]
pub(crate) mod spans;
#[path = "values.rs"]
pub(crate) mod values;

pub(crate) use paths::PathMode as Mode;

pub(crate) fn redact(
    field: &'static str,
    input: &str,
    maximum: usize,
    mode: Mode,
) -> Result<String, ContractError> {
    if mode == Mode::Permission {
        let tokens = shell::shell_tokens(input).map_err(|_| ContractError::ForbiddenContent {
            field: "permissionCommand",
        })?;
        if tokens.iter().any(|token| {
            paths::absolute_path(&token.cooked, mode).is_some()
                || paths::absolute_path(&input[token.start..token.end], mode).is_some()
        }) {
            return Err(ContractError::ForbiddenContent {
                field: "permissionCommand",
            });
        }
    }
    let embedded = if mode == Mode::Diagnostic {
        std::borrow::Cow::Borrowed(input)
    } else {
        embedded::redact(input, maximum, mode, field)?
    };
    let input = embedded.as_ref();
    match mode {
        Mode::Prose => prose::redact_prose(field, input, maximum),
        Mode::CommandOutput | Mode::Permission => command::redact(field, input, maximum, mode),
        Mode::Diagnostic => {
            let output = super::diagnostic::render(input);
            let mut bounded = String::new();
            secrets::append(&mut bounded, &output, maximum, field)?;
            Ok(bounded)
        }
    }
}

#[cfg(test)]
#[path = "corpus.rs"]
mod corpus;

#[cfg(test)]
#[path = "regression_tests.rs"]
mod regression_tests;
