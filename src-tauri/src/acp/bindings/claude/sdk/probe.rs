//! Claude Code reports its models, with each model's effort levels, in its reply to the SDK
//! `initialize` request, which needs no prompt and makes no model call.

use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::admission::{AdmittedClaudeSdkBinding, SDK_ARGS, launch_environment};
use sage_core::limits::LIMITS;
use sage_domain::harness::ModelOption;

use crate::acp::{
    bindings::standard_acp::surface::command_list::{MAX_COMMANDS, parse_acp},
    command_placement::place_commands,
    process::graceful_exit,
    slash_commands::AvailableCommand,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// Startup output before the reply is small; this bounds a misbehaving CLI.
const PROBE_BYTES: u64 = 4 * 1024 * 1024;
const REQUEST_ID: &str = "sage-model-probe";
const SETTINGS_ID: &str = "sage-settings-probe";
const MAX_EFFORT_LEVELS: usize = 16;
const MAX_EFFORT_BYTES: usize = 64;
/// A failed probe is most often a CLI still starting (the first launch after an update can
/// take longer than [`PROBE_TIMEOUT`]), so the person's Retry soon asks again.
const RETRY_AFTER: Duration = Duration::from_secs(10);
/// How long the settings reply may trail the `initialize` one.
const SETTINGS_TIMEOUT: Duration = Duration::from_secs(2);
/// Only the probe's own request runs: no MCP server starts and no hook runs.
const PROBE_ARGS: [&str; 3] = [
    "--strict-mcp-config",
    "--settings",
    r#"{"disableAllHooks":true}"#,
];

enum Probed {
    /// The models, the permission mode the person's settings start a session in, and the
    /// commands Claude Code lists outside any project.
    Models(
        Vec<ModelOption>,
        Option<&'static str>,
        Vec<AvailableCommand>,
    ),
    /// When it failed, and whether a started CLI gave no answer in time (or
    /// refused): only then is the empty listing a failure the person sees.
    /// A build that could not be started, or answered with no models,
    /// offers none as before.
    Failed(Instant, bool),
}

static PROBED: Mutex<Option<([u8; 32], Probed)>> = Mutex::new(None);
/// Held while a probe runs, so concurrent lookups share its outcome.
static PROBING: Mutex<()> = Mutex::new(());

/// `None` when the build could not be read; a later lookup tries again once
/// [`RETRY_AFTER`] has passed.
pub(crate) fn models(binding: &AdmittedClaudeSdkBinding) -> Option<Vec<ModelOption>> {
    let digest = binding.binding_digest();
    let ready = || match lock(&PROBED).as_ref() {
        Some((key, Probed::Models(models, ..))) if *key == digest => Some(Some(models.clone())),
        Some((key, Probed::Failed(at, _))) if *key == digest && at.elapsed() < RETRY_AFTER => {
            Some(None)
        }
        _ => None,
    };
    if let Some(known) = ready() {
        return known;
    }
    let _probing = lock(&PROBING);
    if let Some(known) = ready() {
        return known;
    }
    let started = binding
        .revalidate()
        .ok()
        .and(binding.probe_executable())
        .and_then(|executable| run(executable, &launch_environment()));
    let unanswered = matches!(started, Some(None));
    let probed = started
        .flatten()
        .map(|(reply, settings)| {
            let commands = catalog(&reply);
            (
                model_options(&reply),
                configured_mode(settings.as_ref()),
                commands,
            )
        })
        .filter(|(models, ..)| !models.is_empty());
    *lock(&PROBED) = Some((
        digest,
        probed.clone().map_or_else(
            || Probed::Failed(Instant::now(), unanswered),
            |(models, mode, commands)| Probed::Models(models, mode, commands),
        ),
    ));
    probed.map(|(models, ..)| models)
}

pub(crate) fn known_models(binding: &AdmittedClaudeSdkBinding) -> Option<Vec<ModelOption>> {
    match lock(&PROBED).as_ref() {
        Some((key, Probed::Models(models, ..))) if *key == binding.binding_digest() => {
            Some(models.clone())
        }
        _ => None,
    }
}

/// Whether the latest probe of this build started the CLI and got no answer in time (or a
/// refusal), so an empty listing is a failure to report rather than a build that offers none.
pub(crate) fn listing_failed(binding: &AdmittedClaudeSdkBinding) -> bool {
    matches!(
        lock(&PROBED).as_ref(),
        Some((key, Probed::Failed(_, true))) if *key == binding.binding_digest()
    )
}

/// The permission mode `permissions.defaultMode` in the person's settings names, once a probe
/// has read it; `manual` is Claude Code's other name for `default`.
pub(crate) fn known_start_mode(binding: &AdmittedClaudeSdkBinding) -> Option<&'static str> {
    match lock(&PROBED).as_ref() {
        Some((key, Probed::Models(_, mode, _))) if *key == binding.binding_digest() => *mode,
        _ => None,
    }
}

/// The commands the latest probe of this build read from its `initialize` reply.
pub(crate) fn known_commands(binding: &AdmittedClaudeSdkBinding) -> Option<Vec<AvailableCommand>> {
    match lock(&PROBED).as_ref() {
        Some((key, Probed::Models(_, _, commands))) if *key == binding.binding_digest() => {
            Some(commands.clone())
        }
        _ => None,
    }
}

/// The reply's commands as a session lists them, with retired ones left out. Run from the
/// profile folder, the probe sees no project's own commands.
fn catalog(reply: &Value) -> Vec<AvailableCommand> {
    let Ok(commands) = parse_acp(&super::bridge::catalog_rows(reply)) else {
        return Vec::new();
    };
    place_commands(
        commands.into_iter().take(MAX_COMMANDS).collect(),
        crate::acp::bindings::claude::manifest::known_commands(),
        None,
        &|_| false,
    )
}

fn configured_mode(settings: Option<&Value>) -> Option<&'static str> {
    match settings?
        .pointer("/effective/permissions/defaultMode")?
        .as_str()?
    {
        "manual" => Some("default"),
        mode => crate::acp::bindings::claude::known_mode(mode),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn profile_directory(environment: &[(OsString, OsString)]) -> Option<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    environment
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| PathBuf::from(value))
        .filter(|path| path.is_absolute() && path.is_dir())
}

/// `None` when the CLI could not be started; otherwise the `initialize` reply, and the
/// `get_settings` reply when Claude Code gave one, or `None` when it gave no answer in time.
fn run(
    executable: &Path,
    environment: &[(OsString, OsString)],
) -> Option<Option<(Value, Option<Value>)>> {
    let mut command = Command::new(executable);
    command
        .args(SDK_ARGS)
        .args(PROBE_ARGS)
        .env_clear()
        .envs(environment.iter().cloned())
        .current_dir(profile_directory(environment)?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(sage_process::console::CREATION_FLAGS);
    }
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take();
    let mut stdin = child.stdin.take();
    let (sender, receiver) = mpsc::sync_channel(2);
    if let Some(stdout) = stdout {
        thread::spawn(move || {
            let lines = BufReader::new(stdout.take(PROBE_BYTES)).lines();
            // Reads on past the answers (within the bound) so a CLI still finishing after
            // them never blocks on a full or closed pipe.
            for answer in lines.map_while(Result::ok).filter_map(|line| answer(&line)) {
                let _ = sender.try_send(answer);
            }
        });
    }
    let request = |id: &str, subtype: &str| json!({"type": "control_request", "request_id": id, "request": {"subtype": subtype}});
    let written = stdin.as_mut().is_some_and(|stdin| {
        writeln!(stdin, "{}", request(REQUEST_ID, "initialize"))
            .and_then(|()| writeln!(stdin, "{}", request(SETTINGS_ID, "get_settings")))
            .and_then(|()| stdin.flush())
            .is_ok()
    });
    let reply = written.then(|| collect(&receiver)).flatten();
    // Never a bare kill: closing stdin ends Claude Code's stream-json input,
    // and `graceful_exit` leaves it the grace to save a refreshed sign-in.
    thread::spawn(move || {
        let mut child = child;
        drop(stdin);
        graceful_exit::exit_or_kill(&mut child, Instant::now() + graceful_exit::EXIT_GRACE)
    });
    Some(reply)
}

type Answer = (&'static str, Option<Value>);

/// The `initialize` reply and the settings reply, matched by id in whatever order they come.
/// Settings that have not come [`SETTINGS_TIMEOUT`] after `initialize` stay unknown.
fn collect(answers: &mpsc::Receiver<Answer>) -> Option<(Value, Option<Value>)> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let (mut initialized, mut settings) = (None, None);
    while settings.is_none() || initialized.is_none() {
        let until = match initialized {
            Some(_) => deadline.min(Instant::now() + SETTINGS_TIMEOUT),
            None => deadline,
        };
        let Ok((id, reply)) = answers.recv_timeout(until.saturating_duration_since(Instant::now()))
        else {
            break;
        };
        if id == REQUEST_ID {
            initialized = Some(reply?);
        } else {
            settings = Some(reply);
        }
    }
    initialized.map(|initialized| (initialized, settings.flatten()))
}

/// An answer to one of the probe's requests: its id, and its reply when it succeeded.
fn answer(line: &str) -> Option<(&'static str, Option<Value>)> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let response = &value["response"];
    if value["type"] != "control_response" {
        return None;
    }
    let id = [REQUEST_ID, SETTINGS_ID]
        .into_iter()
        .find(|id| response["request_id"] == *id)?;
    let reply = (response["subtype"] == "success").then(|| response["response"].clone());
    Some((id, reply))
}

/// Claude Code's own `default` entry is the default, or else its first model.
pub(crate) fn model_options(reply: &Value) -> Vec<ModelOption> {
    let entries = reply["models"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let models: Vec<ModelOption> = entries
        .iter()
        .take(LIMITS.wire.models_or_config_entries as usize)
        .filter_map(|entry| {
            let id = entry["value"].as_str().filter(|id| !id.is_empty())?;
            Some(ModelOption {
                id: id.into(),
                name: entry["displayName"].as_str().unwrap_or(id).into(),
                description: entry["description"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                    .map(Into::into),
                is_default: Some(false),
                context_window: None,
                effort_levels: effort_levels(entry),
            })
        })
        .collect();
    let default = models
        .iter()
        .position(|model| model.id == "default")
        .unwrap_or(0);
    models
        .into_iter()
        .enumerate()
        .map(|(index, model)| ModelOption {
            is_default: Some(index == default),
            ..model
        })
        .collect()
}

/// `None` for a model without effort control.
fn effort_levels(entry: &Value) -> Option<Vec<String>> {
    if entry["supportsEffort"].as_bool() != Some(true) {
        return None;
    }
    let levels: Vec<String> = entry["supportedEffortLevels"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|level| !level.is_empty() && level.len() <= MAX_EFFORT_BYTES)
        .take(MAX_EFFORT_LEVELS)
        .map(Into::into)
        .collect();
    (!levels.is_empty()).then_some(levels)
}

/// The effort option a session started on `model` shows, set to `effort`,
/// when the model lists levels and `effort` is Default or one of them.
pub(crate) fn launch_effort_option(
    model: &ModelOption,
    effort: &str,
) -> Option<sage_acp_protocol::session_config::ConfigOption> {
    let levels = model.effort_levels.as_ref()?;
    if effort != super::config::DEFAULT_EFFORT && !model.lists_effort(effort) {
        return None;
    }
    serde_json::from_value(super::config::effort_option(
        levels.iter().map(String::as_str),
        effort,
    ))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that read or write the process-wide probe outcome run one at a time.
    static PROBE_STATE: Mutex<()> = Mutex::new(());

    #[test]
    fn answers_are_told_apart_from_startup_output_and_a_refusal_has_no_reply() {
        let line = |id: &str, subtype: &str| {
            json!({"type": "control_response", "response": {
                "subtype": subtype, "request_id": id,
                "response": {"models": [{"value": "default"}]},
            }})
            .to_string()
        };
        let (id, reply) = answer(&line(REQUEST_ID, "success")).unwrap();
        assert_eq!(id, REQUEST_ID);
        assert_eq!(reply.unwrap()["models"][0]["value"], "default");
        assert_eq!(
            answer(&line(SETTINGS_ID, "error")),
            Some((SETTINGS_ID, None))
        );
        assert!(answer(&line("other", "success")).is_none());
        assert!(answer(&json!({"type": "system", "subtype": "init"}).to_string()).is_none());
        assert!(answer("not json").is_none());
    }

    #[test]
    fn answers_are_matched_by_id_in_either_order_and_a_refused_initialize_fails() {
        let collected = |answers: Vec<Answer>| {
            let (sender, receiver) = mpsc::sync_channel(2);
            for answer in answers {
                sender.send(answer).unwrap();
            }
            collect(&receiver)
        };
        let models = || Some(json!({"models": [{"value": "default"}]}));
        let settings = || Some(json!({"effective": {}}));
        let (reply, read) =
            collected(vec![(SETTINGS_ID, settings()), (REQUEST_ID, models())]).unwrap();
        assert_eq!((reply, read), (models().unwrap(), settings()));
        let (reply, read) = collected(vec![(SETTINGS_ID, None), (REQUEST_ID, models())]).unwrap();
        assert_eq!((reply, read), (models().unwrap(), None));
        assert!(collected(vec![(REQUEST_ID, None), (SETTINGS_ID, settings())]).is_none());
    }

    #[test]
    fn the_settings_default_mode_names_a_known_mode_or_nothing() {
        let settings = |mode: &str| json!({"effective": {"permissions": {"defaultMode": mode}}});
        assert_eq!(configured_mode(Some(&settings("auto"))), Some("auto"));
        assert_eq!(configured_mode(Some(&settings("manual"))), Some("default"));
        assert_eq!(configured_mode(Some(&settings("plan"))), Some("plan"));
        assert_eq!(configured_mode(Some(&settings("someFutureMode"))), None);
        assert_eq!(configured_mode(Some(&json!({"effective": {}}))), None);
        assert_eq!(configured_mode(None), None);
    }

    #[test]
    fn listed_models_keep_their_order_and_claude_codes_default_is_the_default() {
        let reply = json!({"models": [
            {"value": "opus", "displayName": "Opus", "description": "Most capable"},
            {"value": "default", "displayName": "Default (recommended)", "description": ""},
            {"value": "", "displayName": "Nameless"},
            {"displayName": "No value"},
            {"value": "haiku"},
        ]});
        let models = model_options(&reply);
        let rows: Vec<_> = models
            .iter()
            .map(|m| {
                (
                    m.id.as_str(),
                    m.name.as_str(),
                    m.description.as_deref(),
                    m.is_default,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("opus", "Opus", Some("Most capable"), Some(false)),
                ("default", "Default (recommended)", None, Some(true)),
                ("haiku", "haiku", None, Some(false)),
            ]
        );
        let first = model_options(&json!({"models": [{"value": "a"}, {"value": "b"}]}));
        assert_eq!(first[0].is_default, Some(true));
        assert!(model_options(&json!({})).is_empty());
    }

    #[test]
    fn a_model_lists_its_thinking_levels_only_when_it_supports_effort() {
        let models = model_options(&json!({"models": [
            {"value": "default", "supportsEffort": true,
             "supportedEffortLevels": ["low", "", 3, "high"]},
            {"value": "haiku", "supportsEffort": false, "supportedEffortLevels": ["low"]},
            {"value": "opus", "supportsEffort": true, "supportedEffortLevels": []},
        ]}));
        let levels: Vec<_> = models.iter().map(|m| m.effort_levels.clone()).collect();
        assert_eq!(
            levels,
            [Some(vec!["low".to_owned(), "high".to_owned()]), None, None]
        );
    }

    #[test]
    fn a_start_takes_only_a_model_and_level_the_probed_build_listed() {
        let _state = lock(&PROBE_STATE);
        let binding = AdmittedClaudeSdkBinding::for_test_with_digest([9; 32]);
        let models = model_options(&json!({"models": [
            {"value": "default", "supportsEffort": true, "supportedEffortLevels": ["low", "high"]},
            {"value": "haiku"},
        ]}));
        *lock(&PROBED) = Some((
            binding.binding_digest(),
            Probed::Models(models, None, Vec::new()),
        ));
        let known = known_models(&binding).unwrap();
        let launched = |model| ModelOption::launched(&known, model);
        assert_eq!(launched(Some("haiku")).unwrap().id, "haiku");
        assert!(launched(Some("opus")).is_none());
        let default = launched(None).unwrap();
        assert_eq!(default.id, "default");
        let haiku = launched(Some("haiku")).unwrap();
        let offered = |model, effort| {
            launch_effort_option(model, effort).map(|option| serde_json::to_value(option).unwrap())
        };
        // The option offers every level the model lists, as the session will.
        let high = offered(default, "high").unwrap();
        assert_eq!(high["id"], "effort");
        assert_eq!(high["currentValue"], "high");
        assert_eq!(
            high["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|choice| choice["value"].clone())
                .collect::<Vec<_>>(),
            [json!("default"), json!("low"), json!("high")]
        );
        assert_eq!(
            offered(default, "default").unwrap()["currentValue"],
            "default"
        );
        assert!(offered(default, "max").is_none());
        assert!(offered(haiku, "low").is_none());
        assert!(offered(haiku, "default").is_none());
    }

    #[test]
    fn a_build_that_cannot_be_probed_offers_nothing_and_is_not_probed_again_at_once() {
        let _state = lock(&PROBE_STATE);
        let binding = AdmittedClaudeSdkBinding::for_test();
        assert!(models(&binding).is_none());
        assert!(matches!(
            lock(&PROBED).as_ref(),
            Some((key, Probed::Failed(_, false))) if *key == binding.binding_digest()
        ));
        assert!(models(&binding).is_none());
        assert!(known_models(&binding).is_none());
        // Nothing was started (here the build cannot even be read), so there is no
        // listing failure to report: the harness offers no models, as before.
        assert!(!listing_failed(&binding));
        // A started CLI that gave no answer in time is reported, for that build only.
        *lock(&PROBED) = Some((
            binding.binding_digest(),
            Probed::Failed(Instant::now(), true),
        ));
        assert!(listing_failed(&binding));
        let other = AdmittedClaudeSdkBinding::for_test_with_digest([8; 32]);
        assert!(!listing_failed(&other));
    }

    #[cfg(unix)]
    #[test]
    fn a_cli_that_answers_initialize_is_read_and_one_that_hangs_is_abandoned() {
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!("sage-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("claude");
        sage_test_support::test_executable::write(
            &script,
            format!(
                "#!/bin/sh\nread line\nread line\necho '{}'\nsleep 30\n",
                json!({"type": "control_response", "response": {
                    "subtype": "success", "request_id": REQUEST_ID,
                    "response": {"models": [{"value": "default", "displayName": "Default"}]},
                }})
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let environment = vec![
            (OsString::from("HOME"), directory.clone().into_os_string()),
            (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        ];
        let started = std::time::Instant::now();
        let (reply, settings) = run(&script, &environment).unwrap().unwrap();
        assert_eq!(reply["models"][0]["value"], "default");
        assert!(settings.is_none(), "unanswered settings stay unknown");
        assert!(started.elapsed() < Duration::from_secs(10));
        // A CLI that starts but never answers is abandoned at the deadline as unanswered;
        // one that cannot start is told apart from it.
        let silent = directory.join("silent");
        sage_test_support::test_executable::write(&silent, "#!/bin/sh\nsleep 60\n").unwrap();
        std::fs::set_permissions(&silent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(run(&silent, &environment), Some(None));
        assert!(started.elapsed() < PROBE_TIMEOUT + Duration::from_secs(5));
        assert!(run(&directory.join("missing"), &environment).is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_probe_keeps_the_reply_commands_a_session_would_list_and_drops_retired_ones() {
        let reply = json!({"commands": [
            {"name": "agents", "description": "(removed) Use the Agent tool", "builtin": true},
            {"name": "compact", "description": "Compact the conversation", "builtin": true},
            {"name": "review-pr", "description": "Review a pull request", "argumentHint": "<pr>"},
            {"name": "deploy:ship", "description": "Ship it"},
        ]});
        let listed = catalog(&reply);
        let names: Vec<&str> = listed.iter().map(|command| command.name.as_str()).collect();
        assert_eq!(names, ["compact", "review-pr", "deploy:ship"]);
        assert_eq!(listed[1].input_hint.as_deref(), Some("<pr>"));
        assert!(catalog(&json!({})).is_empty());
    }
}
