use std::time::{Duration, Instant};

use super::{RendererRecreationFrontendPacket, RendererRecreationPairError};
#[cfg(test)]
use super::{RendererReload, RendererReloadError};
use crate::acp::service::renderer_recreation_packet::RendererRecreationPacketAdmitError;
use tracing::info;

const MAIN_WINDOW_LABEL: &str = "main";
const FRONTEND_PACKET_EVAL_TIMEOUT: Duration = Duration::from_secs(5);
const FRONTEND_PACKET_RETRY_DELAY: Duration = Duration::from_millis(25);
pub(super) const RENDERER_RECREATION_PACKET_EVAL: &str = r#"(function(){try{var collect=window.__SAGE_COLLECT_RENDERER_RECREATION_PACKET__;if(typeof collect!=="function")return null;return collect();}catch(_error){return null;}})()"#;
const PACKET_KEYS: [&str; 5] = [
    "schemaVersion",
    "liveRegions",
    "pagehideListeners",
    "focusOutcome",
    "projectionSlots",
];

#[cfg(test)]
pub(super) fn reload_labeled_main<E>(
    label: &str,
    reload: impl FnOnce() -> Result<(), E>,
) -> Result<(), RendererReloadError> {
    if label != MAIN_WINDOW_LABEL {
        return Err(RendererReloadError::Failed);
    }
    reload().map_err(|_| RendererReloadError::Failed)
}

pub(super) fn read_labeled_main_frontend_packet<E>(
    label: &str,
    start_eval: impl Fn(&str, Box<dyn Fn(String) + Send + 'static>) -> Result<(), E>,
    timeout: Duration,
) -> Result<String, RendererRecreationPairError> {
    if label != MAIN_WINDOW_LABEL {
        return Err(RendererRecreationPairError::FrontendRead);
    }
    let deadline = Instant::now() + timeout;
    let mut logged_unavailable = false;
    loop {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        start_eval(
            RENDERER_RECREATION_PACKET_EVAL,
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        )
        .map_err(|_| RendererRecreationPairError::FrontendRead)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let raw = rx
            .recv_timeout(remaining)
            .map_err(|_| RendererRecreationPairError::FrontendRead)?;
        if !raw.is_empty() && raw != "null" && raw != "undefined" {
            let packet = unwrap_tauri_callback_packet(&raw);
            log_frontend_packet_shape(&raw, &packet);
            return Ok(packet);
        }
        if !logged_unavailable {
            log_frontend_packet_shape(&raw, &raw);
            logged_unavailable = true;
        }
        if Instant::now() >= deadline {
            return Err(RendererRecreationPairError::Frontend(
                RendererRecreationPacketAdmitError::InvalidJson,
            ));
        }
        std::thread::sleep(
            FRONTEND_PACKET_RETRY_DELAY.min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn unwrap_tauri_callback_packet(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| raw.to_owned())
}

fn log_frontend_packet_shape(callback: &str, packet: &str) {
    let callback_shape = serde_json::from_str::<serde_json::Value>(callback)
        .map(|value| json_shape(&value))
        .unwrap_or_else(|error| format!("invalid-json:{error}"));
    let packet_shape = serde_json::from_str::<serde_json::Value>(packet)
        .map(|value| json_shape(&value))
        .unwrap_or_else(|error| format!("invalid-json:{error}"));
    info!(
        callback_bytes = callback.len(),
        callback_shape,
        packet_bytes = packet.len(),
        packet_shape,
        "ACP renderer recreation frontend packet observed"
    );
}

fn json_shape(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(object) => {
            let known_keys = PACKET_KEYS
                .iter()
                .copied()
                .filter(|key| object.contains_key(*key))
                .collect::<Vec<_>>();
            format!(
                "object:known_keys={known_keys:?}:unknown_key_count={}",
                object.len().saturating_sub(known_keys.len())
            )
        }
        serde_json::Value::String(_) => "string".to_owned(),
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(_) => "boolean".to_owned(),
        serde_json::Value::Number(_) => "number".to_owned(),
        serde_json::Value::Array(values) => format!("array:length={}", values.len()),
    }
}

#[allow(dead_code)] // Production adapter; packaged observer still pending.
pub(crate) struct MainWindowRendererReload<'a> {
    window: &'a tauri::WebviewWindow,
}

#[allow(dead_code)]
impl<'a> MainWindowRendererReload<'a> {
    pub(crate) fn new(window: &'a tauri::WebviewWindow) -> Self {
        Self { window }
    }
}

#[cfg(test)]
impl RendererReload for MainWindowRendererReload<'_> {
    fn reload_main(&self) -> Result<(), RendererReloadError> {
        reload_labeled_main(self.window.label(), || self.window.reload())
    }
}

impl RendererRecreationFrontendPacket for MainWindowRendererReload<'_> {
    fn after_frontend_packet(&self) -> Result<String, RendererRecreationPairError> {
        read_labeled_main_frontend_packet(
            self.window.label(),
            |script, callback| self.window.eval_with_callback(script, callback),
            FRONTEND_PACKET_EVAL_TIMEOUT,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RENDERER_RECREATION_PACKET_EVAL, read_labeled_main_frontend_packet, reload_labeled_main,
    };
    use crate::acp::service::renderer_recreation_orchestrator::{
        RendererRecreationPairError, RendererReloadError,
    };
    use crate::acp::service::renderer_recreation_packet::{
        RendererRecreationPacketAdmitError, admit_renderer_recreation_frontend_packet,
    };
    use std::time::Duration;

    const ANNOUNCEMENT_PACKET: &str = "{\"schemaVersion\":1,\"liveRegions\":{\"polite\":1,\"assertive\":1},\"pagehideListeners\":1,\"focusOutcome\":\"exact-sage-id\",\"projectionSlots\":1}";

    #[test]
    fn eval_script_matches_the_frontend_constant() {
        let frontend =
            include_str!("../../../../../src/features/chat/acp/rendererRecreationPacket.ts");
        assert!(
            frontend.contains(RENDERER_RECREATION_PACKET_EVAL),
            "native observer eval script must match RENDERER_RECREATION_PACKET_EVAL_SCRIPT"
        );
    }

    #[test]
    fn non_main_label_never_invokes_reload() {
        let calls = std::cell::Cell::new(0_u8);
        assert_eq!(
            reload_labeled_main("preview", || {
                calls.set(calls.get().saturating_add(1));
                Ok::<(), &str>(())
            }),
            Err(RendererReloadError::Failed)
        );
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn main_label_invokes_reload_once() {
        let calls = std::cell::Cell::new(0_u8);
        reload_labeled_main("main", || {
            calls.set(calls.get().saturating_add(1));
            Ok::<(), &str>(())
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn main_label_reload_error_maps_to_failed() {
        assert_eq!(
            reload_labeled_main("main", || Err::<(), &str>("webview")),
            Err(RendererReloadError::Failed)
        );
    }

    #[test]
    fn non_main_label_never_starts_frontend_eval() {
        let calls = std::cell::Cell::new(0_u8);
        assert_eq!(
            read_labeled_main_frontend_packet(
                "preview",
                |_, _| {
                    calls.set(calls.get().saturating_add(1));
                    Ok::<(), &str>(())
                },
                Duration::from_millis(20),
            ),
            Err(RendererRecreationPairError::FrontendRead)
        );
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn main_label_eval_returns_the_callback_json() {
        let calls = std::cell::Cell::new(0_u8);
        let packet = read_labeled_main_frontend_packet(
            "main",
            |script, callback| {
                assert_eq!(script, RENDERER_RECREATION_PACKET_EVAL);
                calls.set(calls.get().saturating_add(1));
                callback(ANNOUNCEMENT_PACKET.to_string());
                Ok::<(), &str>(())
            },
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(packet, ANNOUNCEMENT_PACKET);
    }

    #[test]
    fn main_label_unwraps_tauri_json_string_callback_packet() {
        let tauri_callback = serde_json::to_string(ANNOUNCEMENT_PACKET).unwrap();
        let packet = read_labeled_main_frontend_packet(
            "main",
            |_, callback| {
                callback(tauri_callback.clone());
                Ok::<(), &str>(())
            },
            Duration::from_secs(1),
        )
        .unwrap();

        assert_eq!(packet, ANNOUNCEMENT_PACKET);
    }

    #[test]
    fn main_label_retries_until_the_rehydrated_collector_is_available() {
        let calls = std::cell::Cell::new(0_u8);
        let tauri_callback = serde_json::to_string(ANNOUNCEMENT_PACKET).unwrap();
        let packet = read_labeled_main_frontend_packet(
            "main",
            |_, callback| {
                let call = calls.get();
                calls.set(call.saturating_add(1));
                callback(if call == 0 {
                    "null".to_owned()
                } else {
                    tauri_callback.clone()
                });
                Ok::<(), &str>(())
            },
            Duration::from_secs(1),
        )
        .unwrap();

        assert_eq!(calls.get(), 2);
        assert_eq!(packet, ANNOUNCEMENT_PACKET);
    }

    #[test]
    fn tauri_json_string_callback_leaves_genuine_packet_garbage_for_native_rejection() {
        let tauri_callback = serde_json::to_string("genuine packet garbage").unwrap();
        let packet = read_labeled_main_frontend_packet(
            "main",
            |_, callback| {
                callback(tauri_callback.clone());
                Ok::<(), &str>(())
            },
            Duration::from_secs(1),
        )
        .unwrap();

        assert_eq!(
            admit_renderer_recreation_frontend_packet(&packet),
            Err(RendererRecreationPacketAdmitError::InvalidJson)
        );
    }

    #[test]
    fn js_null_or_empty_eval_result_is_invalid_json() {
        for raw in ["", "null", "undefined"] {
            assert_eq!(
                read_labeled_main_frontend_packet(
                    "main",
                    |_, callback| {
                        callback(raw.to_string());
                        Ok::<(), &str>(())
                    },
                    Duration::from_secs(1),
                ),
                Err(RendererRecreationPairError::Frontend(
                    RendererRecreationPacketAdmitError::InvalidJson
                ))
            );
        }
    }

    #[test]
    fn eval_start_error_is_frontend_read() {
        assert_eq!(
            read_labeled_main_frontend_packet(
                "main",
                |_, _| Err::<(), &str>("webview"),
                Duration::from_secs(1),
            ),
            Err(RendererRecreationPairError::FrontendRead)
        );
    }

    #[test]
    fn missing_eval_callback_times_out_as_frontend_read() {
        assert_eq!(
            read_labeled_main_frontend_packet(
                "main",
                |_, _| Ok::<(), &str>(()),
                Duration::from_millis(20),
            ),
            Err(RendererRecreationPairError::FrontendRead)
        );
    }

    #[test]
    fn production_generate_handler_has_no_reload_command() {
        let source = include_str!("../../../ipc_commands.rs");
        let handler = source
            .split("generate_handler![")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("production generate_handler list");
        assert!(
            !handler.contains("reload"),
            "generate_handler must not register a reload command"
        );
        assert!(
            !handler.contains("observe_renderer_recreation"),
            "generate_handler must not register observe_renderer_recreation"
        );
        assert!(
            !handler.contains("eval_with_callback"),
            "generate_handler must not register an eval packet command"
        );
    }
}
