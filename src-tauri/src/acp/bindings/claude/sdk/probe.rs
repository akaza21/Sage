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

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// Startup output before the reply is small; this bounds a misbehaving CLI.
const PROBE_BYTES: u64 = 4 * 1024 * 1024;
const REQUEST_ID: &str = "sage-model-probe";
const MAX_EFFORT_LEVELS: usize = 16;
const MAX_EFFORT_BYTES: usize = 64;
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Only the probe's own request runs: no MCP server starts and no hook runs.
const PROBE_ARGS: [&str; 3] = [
    "--strict-mcp-config",
    "--settings",
    r#"{"disableAllHooks":true}"#,
];

enum Probed {
    Models(Vec<ModelOption>),
    Failed(Instant),
}

static PROBED: Mutex<Option<([u8; 32], Probed)>> = Mutex::new(None);
/// Held while a probe runs, so concurrent lookups share its outcome.
static PROBING: Mutex<()> = Mutex::new(());

/// `None` when the build could not be read; a later lookup tries again once
/// [`RETRY_AFTER`] has passed.
pub(crate) fn models(binding: &AdmittedClaudeSdkBinding) -> Option<Vec<ModelOption>> {
    let digest = binding.binding_digest();
    let ready = || match lock(&PROBED).as_ref() {
        Some((key, Probed::Models(models))) if *key == digest => Some(Some(models.clone())),
        Some((key, Probed::Failed(at))) if *key == digest && at.elapsed() < RETRY_AFTER => {
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
    let models = binding
        .revalidate()
        .ok()
        .and(binding.probe_executable())
        .and_then(|executable| run(executable, &launch_environment()))
        .map(|reply| model_options(&reply))
        .filter(|models| !models.is_empty());
    *lock(&PROBED) = Some((
        digest,
        models
            .clone()
            .map_or_else(|| Probed::Failed(Instant::now()), Probed::Models),
    ));
    models
}

pub(crate) fn known_models(binding: &AdmittedClaudeSdkBinding) -> Option<Vec<ModelOption>> {
    match lock(&PROBED).as_ref() {
        Some((key, Probed::Models(models))) if *key == binding.binding_digest() => {
            Some(models.clone())
        }
        _ => None,
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

fn run(executable: &Path, environment: &[(OsString, OsString)]) -> Option<Value> {
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
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take();
    let mut stdin = child.stdin.take();
    let (sender, receiver) = mpsc::sync_channel(1);
    if let Some(stdout) = stdout {
        thread::spawn(move || {
            let _ = sender.send(read_reply(BufReader::new(stdout.take(PROBE_BYTES))));
        });
    }
    let request = json!({
        "type": "control_request",
        "request_id": REQUEST_ID,
        "request": {"subtype": "initialize"},
    });
    let written = stdin.as_mut().is_some_and(|stdin| {
        writeln!(stdin, "{request}")
            .and_then(|()| stdin.flush())
            .is_ok()
    });
    let reply = written
        .then(|| receiver.recv_timeout(PROBE_TIMEOUT).ok().flatten())
        .flatten();
    drop(stdin);
    let _ = child.kill();
    thread::spawn(move || {
        let _ = child.wait();
    });
    reply
}

fn read_reply(reader: impl BufRead) -> Option<Value> {
    for line in reader.lines() {
        let Ok(value) = serde_json::from_str::<Value>(&line.ok()?) else {
            continue;
        };
        let response = &value["response"];
        if value["type"] == "control_response" && response["request_id"] == REQUEST_ID {
            return (response["subtype"] == "success").then(|| response["response"].clone());
        }
    }
    None
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

pub(crate) fn launch_effort_option(model: &ModelOption, effort: &str) -> Option<&'static str> {
    (model.effort_levels.is_some()
        && (effort == super::config::DEFAULT_EFFORT || model.lists_effort(effort)))
    .then_some(super::config::EFFORT_ID)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that read or write the process-wide probe outcome run one at a time.
    static PROBE_STATE: Mutex<()> = Mutex::new(());

    #[test]
    fn the_reply_is_found_among_startup_output_and_a_refusal_is_not_a_list() {
        let output = format!(
            "not json\n{}\n{}\n",
            json!({"type": "system", "subtype": "init"}),
            json!({"type": "control_response", "response": {
                "subtype": "success", "request_id": REQUEST_ID,
                "response": {"models": [{"value": "default"}]},
            }}),
        );
        let reply = read_reply(output.as_bytes()).unwrap();
        assert_eq!(reply["models"][0]["value"], "default");
        let refused = json!({"type": "control_response", "response": {
            "subtype": "error", "request_id": REQUEST_ID, "error": "no",
        }})
        .to_string();
        assert!(read_reply(refused.as_bytes()).is_none());
        assert!(read_reply(&b"{}\n"[..]).is_none());
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
        *lock(&PROBED) = Some((binding.binding_digest(), Probed::Models(models)));
        let known = known_models(&binding).unwrap();
        let launched = |model| ModelOption::launched(&known, model);
        assert_eq!(launched(Some("haiku")).unwrap().id, "haiku");
        assert!(launched(Some("opus")).is_none());
        let default = launched(None).unwrap();
        assert_eq!(default.id, "default");
        let haiku = launched(Some("haiku")).unwrap();
        assert_eq!(launch_effort_option(default, "high"), Some("effort"));
        assert_eq!(launch_effort_option(default, "default"), Some("effort"));
        assert_eq!(launch_effort_option(default, "max"), None);
        assert_eq!(launch_effort_option(haiku, "low"), None);
        assert_eq!(launch_effort_option(haiku, "default"), None);
    }

    #[test]
    fn a_build_that_cannot_be_probed_offers_nothing_and_is_not_probed_again_at_once() {
        let _state = lock(&PROBE_STATE);
        let binding = AdmittedClaudeSdkBinding::for_test();
        assert!(models(&binding).is_none());
        assert!(matches!(
            lock(&PROBED).as_ref(),
            Some((key, Probed::Failed(_))) if *key == binding.binding_digest()
        ));
        assert!(models(&binding).is_none());
        assert!(known_models(&binding).is_none());
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
                "#!/bin/sh\nread line\necho '{}'\nsleep 30\n",
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
        let reply = run(&script, &environment).unwrap();
        assert_eq!(reply["models"][0]["value"], "default");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(run(&directory.join("missing"), &environment).is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
