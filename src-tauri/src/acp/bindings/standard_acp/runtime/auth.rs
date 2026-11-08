//! ACP sign-in: the agent lists `authMethods` in its `initialize` result and
//! refuses `session/new` with `auth_required` until the client calls
//! `authenticate`. Sage authenticates once, with the first method that
//! needs no input from the user, then opens the session again. Methods that
//! need input (a terminal login, an environment variable) become a notice
//! naming each method, since Sage cannot complete them for the user.

use serde_json::{Value, json};

use crate::acp::bindings::adapter::{AdapterBatch, AdapterRuntimeError, RefusalDetail};

use super::{PendingRequest, StandardAcpRuntime};

const MAX_METHODS: usize = 8;
const MAX_FIELD_CHARS: usize = 64;
const MAX_TERMINAL_ARGS: usize = 8;
const NOTICE_CHARS: usize = 480;
const REMEDIATION: &str = "Complete the agent's sign-in outside Sage, then try again.";

#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthKind {
    /// The agent signs in by itself once asked (`type` absent or `agent`).
    Agent,
    EnvVar(Option<String>),
    Terminal(Vec<String>),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthMethod {
    id: String,
    name: String,
    kind: AuthKind,
}

#[derive(Debug, Default)]
pub(super) struct SignIn {
    methods: Vec<AuthMethod>,
    attempted: bool,
    root: Option<String>,
}

impl SignIn {
    pub(super) fn remember_root(&mut self, root: &str) {
        self.root = Some(root.to_owned());
    }

    /// Reads the bounded `authMethods` list from an `initialize` response.
    pub(super) fn accept_initialize(&mut self, response: &Value) {
        self.methods = response
            .pointer("/result/authMethods")
            .and_then(Value::as_array)
            .map(|methods| {
                methods
                    .iter()
                    .take(MAX_METHODS)
                    .filter_map(auth_method)
                    .collect()
            })
            .unwrap_or_default();
    }

    fn no_input_method(&self) -> Option<&AuthMethod> {
        (!self.attempted)
            .then(|| {
                self.methods
                    .iter()
                    .find(|method| method.kind == AuthKind::Agent)
            })
            .flatten()
    }

    /// One sentence naming every offered method and what it needs.
    pub(super) fn notice(&self, label: &str) -> String {
        let mut notice = if self.methods.is_empty() {
            format!("{label} needs sign-in and offered no sign-in method.")
        } else {
            let offered = self
                .methods
                .iter()
                .map(describe)
                .collect::<Vec<_>>()
                .join("; ");
            format!("{label} needs sign-in. It offers: {offered}.")
        };
        if notice.chars().count() > NOTICE_CHARS {
            notice = notice.chars().take(NOTICE_CHARS - 1).collect();
            notice.push('…');
        }
        notice
    }
}

fn describe(method: &AuthMethod) -> String {
    match &method.kind {
        AuthKind::Agent => format!("{} (Sage signs in for you)", method.name),
        AuthKind::EnvVar(Some(name)) => format!(
            "{} (set {name} and add it to the agent's passed-through variables)",
            method.name
        ),
        AuthKind::EnvVar(None) => format!("{} (an environment variable)", method.name),
        AuthKind::Terminal(args) if args.is_empty() => {
            format!("{} (run the agent in a terminal)", method.name)
        }
        AuthKind::Terminal(args) => format!(
            "{} (run the agent in a terminal with {})",
            method.name,
            args.join(" ")
        ),
        AuthKind::Other => method.name.clone(),
    }
}

fn auth_method(value: &Value) -> Option<AuthMethod> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| bounded(text))
            .map(str::to_owned)
    };
    let kind = match value.get("type").and_then(Value::as_str) {
        None | Some("agent") => AuthKind::Agent,
        Some("env_var") => AuthKind::EnvVar(text("varName").or_else(|| {
            value
                .pointer("/vars/0/name")
                .and_then(Value::as_str)
                .filter(|text| bounded(text))
                .map(str::to_owned)
        })),
        Some("terminal") => AuthKind::Terminal(
            value
                .get("args")
                .and_then(Value::as_array)
                .map(|args| {
                    args.iter()
                        .take(MAX_TERMINAL_ARGS)
                        .filter_map(Value::as_str)
                        .filter(|arg| bounded(arg))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Some(_) => AuthKind::Other,
    };
    Some(AuthMethod {
        id: text("id")?,
        name: text("name")?,
        kind,
    })
}

fn bounded(text: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= MAX_FIELD_CHARS
        && !text.chars().any(char::is_control)
}

impl StandardAcpRuntime {
    /// `session/new` was refused for sign-in.
    pub(super) fn sign_in_required(&mut self) -> Result<AdapterBatch, AdapterRuntimeError> {
        if let Some(method_id) = self.sign_in.no_input_method().map(|m| m.id.clone()) {
            self.sign_in.attempted = true;
            let id = self.next_request_id.saturating_add(1);
            let egress = self.reserve(
                PendingRequest::Authenticate,
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "authenticate",
                    "params": {"methodId": method_id}
                }),
            )?;
            return AdapterBatch::new(Vec::new(), vec![egress]);
        }
        self.refuse_sign_in()
    }

    pub(super) fn authenticate_response(
        &mut self,
        value: Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if value.get("error").is_some() || !value.get("result").is_some_and(Value::is_object) {
            return self.refuse_sign_in();
        }
        let root = self
            .sign_in
            .root
            .clone()
            .ok_or(AdapterRuntimeError::Frame)?;
        self.handshake
            .reopen_session_as(self.next_request_id.saturating_add(1))
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let egress = self.new_session(root)?;
        AdapterBatch::new(Vec::new(), vec![egress])
    }

    fn refuse_sign_in(&mut self) -> Result<AdapterBatch, AdapterRuntimeError> {
        let notice = self.sign_in.notice(self.profile.agent_label);
        self.source.record_sign_in(&notice);
        self.refusal_detail = RefusalDetail::new(Some(&notice), Some(REMEDIATION));
        Err(AdapterRuntimeError::SignInRequired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign_in(methods: Value) -> SignIn {
        let mut sign_in = SignIn::default();
        sign_in.accept_initialize(&json!({"id": 1, "result": {"authMethods": methods}}));
        sign_in
    }

    #[test]
    fn only_a_method_without_input_is_authenticated_and_only_once() {
        let mut offered = sign_in(json!([
            {"id": "term", "name": "Terminal login", "type": "terminal", "args": ["--login"]},
            {"id": "oauth", "name": "Browser"}
        ]));
        assert_eq!(
            offered.no_input_method().map(|m| m.id.as_str()),
            Some("oauth")
        );
        offered.attempted = true;
        assert!(offered.no_input_method().is_none());
        let input_only = sign_in(json!([
            {"id": "key", "name": "API key", "type": "env_var", "varName": "AGENT_KEY"}
        ]));
        assert!(input_only.no_input_method().is_none());
    }

    #[test]
    fn the_notice_names_each_method_and_what_it_needs() {
        let offered = sign_in(json!([
            {"id": "term", "name": "Terminal login", "type": "terminal", "args": ["auth", "login"]},
            {"id": "key", "name": "API key", "type": "env_var", "varName": "AGENT_KEY"}
        ]));
        assert_eq!(
            offered.notice("My agent"),
            "My agent needs sign-in. It offers: Terminal login (run the agent in a terminal with auth login); API key (set AGENT_KEY and add it to the agent's passed-through variables)."
        );
        assert_eq!(
            sign_in(json!([])).notice("My agent"),
            "My agent needs sign-in and offered no sign-in method."
        );
    }

    #[test]
    fn methods_are_bounded_and_unprintable_text_is_dropped() {
        let many: Vec<Value> = (0..20)
            .map(|index| json!({"id": format!("m{index}"), "name": format!("Method {index}")}))
            .collect();
        assert_eq!(sign_in(Value::Array(many)).methods.len(), MAX_METHODS);
        let hostile = sign_in(json!([
            {"id": "a", "name": "line\nbreak"},
            {"id": "b", "name": "x".repeat(65)},
            {"id": "c", "name": "Fine"}
        ]));
        assert_eq!(hostile.methods.len(), 1);
        let long = sign_in(Value::Array(
            (0..8)
                .map(|index| json!({"id": format!("m{index}"), "name": "n".repeat(64), "type": "terminal", "args": ["a".repeat(64)]}))
                .collect(),
        ));
        assert!(long.notice("Agent").chars().count() <= NOTICE_CHARS);
    }
}
