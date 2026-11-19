use super::connection::{self, SessionEvent};
use super::protocol::{MAX_LISTENERS, Outbound, Request, admits_command};
use super::transport::Session;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use tauri::{
    Listener, Manager,
    ipc::{CallbackFn, InvokeBody},
    webview::InvokeRequest,
};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;

enum Subscription {
    Native(tauri::EventId),
    CloseRequested,
}

struct Listeners {
    app: tauri::AppHandle,
    ids: HashMap<u32, Subscription>,
}
impl Drop for Listeners {
    fn drop(&mut self) {
        for (_, subscription) in self.ids.drain() {
            if let Subscription::Native(id) = subscription {
                self.app.unlisten(id);
            }
        }
    }
}

impl Listeners {
    fn forward_close(&self, session: &Session, output: &Outbound) {
        let subscribed = self
            .ids
            .values()
            .any(|subscription| matches!(subscription, Subscription::CloseRequested));
        if subscribed && session.forward_close_request() {
            for (&handler, subscription) in &self.ids {
                if matches!(subscription, Subscription::CloseRequested) {
                    output.event("tauri://close-requested", handler, "null");
                }
            }
        }
    }
}

pub(super) async fn serve(
    socket: WebSocketStream<TcpStream>,
    app: tauri::AppHandle,
    session: Session,
) {
    let mut listeners = Listeners {
        app: app.clone(),
        ids: HashMap::new(),
    };
    connection::run(socket, session, |event, session, output| match event {
        SessionEvent::CloseRequested => listeners.forward_close(session, output),
        SessionEvent::Request(request, body_bytes) => {
            if !admits_command(&request.cmd) {
                output.error(request.id, "command is not admitted by the browser bridge");
            } else if request.cmd == "plugin:event|listen" {
                listen(&mut listeners, request, output, session);
            } else if request.cmd == "plugin:event|unlisten" {
                unlisten(&mut listeners, request, output);
            } else {
                dispatch(&app, request, body_bytes, output.clone(), session);
            }
        }
    })
    .await;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListenArgs {
    event: String,
    target: ListenTarget,
    handler: u32,
}

#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum ListenTarget {
    Any,
    AnyLabel { label: String },
    Window { label: String },
    Webview { label: String },
    WebviewWindow { label: String },
}

impl ListenTarget {
    /// The page stands in for the main webview, so it hears what that webview
    /// would: global events and events addressed to `main`, never another window's.
    fn reaches_main(&self) -> bool {
        match self {
            Self::Any => true,
            Self::AnyLabel { label }
            | Self::Window { label }
            | Self::Webview { label }
            | Self::WebviewWindow { label } => label == "main",
        }
    }
}

fn listen(listeners: &mut Listeners, request: Request, output: &Outbound, session: &Session) {
    let Some(args) = serde_json::from_value::<ListenArgs>(request.args)
        .ok()
        .filter(|args| args.target.reaches_main())
    else {
        output.error(request.id, "invalid event listener");
        return;
    };
    let Some(main) = listeners.app.get_webview_window("main") else {
        output.error(request.id, "main window unavailable");
        return;
    };
    if args.event.is_empty()
        || args.event.len() > 128
        || !args
            .event
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-/:_".contains(&byte))
        || listeners.ids.len() >= MAX_LISTENERS
        || listeners.ids.contains_key(&args.handler)
    {
        output.error(request.id, "event listener limit or invalid event name");
        return;
    }
    let id = if args.event == "tauri://close-requested" {
        // Native close requests use a latch until the browser's existing close
        // handler subscribes; registering a Tauri listener too would deliver twice.
        Subscription::CloseRequested
    } else {
        let outbound = output.clone();
        let name = args.event.clone();
        Subscription::Native(main.listen(args.event, move |event| {
            outbound.event(&name, args.handler, event.payload());
        }))
    };
    // The callback id is also the bridge listener id, so events may precede the acknowledgement.
    listeners.ids.insert(args.handler, id);
    output.send(json!({"id":request.id, "ok":args.handler}));
    listeners.forward_close(session, output);
}

fn unlisten(listeners: &mut Listeners, request: Request, output: &Outbound) {
    let id = request
        .args
        .get("eventId")
        .and_then(Value::as_u64)
        .and_then(|id| u32::try_from(id).ok());
    if let Some(id) = id.and_then(|id| listeners.ids.remove(&id)) {
        if let Subscription::Native(id) = id {
            listeners.app.unlisten(id);
        }
        output.send(json!({"id":request.id, "ok":null}));
    } else {
        output.error(request.id, "unknown event listener");
    }
}

fn dispatch(
    app: &tauri::AppHandle,
    request: Request,
    body_bytes: usize,
    output: Outbound,
    session: &Session,
) {
    let Some(reply) = session.admit(request.id, body_bytes, &output) else {
        return;
    };
    let Some(webview) = app.get_webview("main") else {
        output.error(request.id, "main webview unavailable");
        return;
    };
    let Ok(url) = webview.url() else {
        output.error(request.id, "main webview URL unavailable");
        return;
    };
    let invoke = InvokeRequest {
        cmd: request.cmd,
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url,
        body: InvokeBody::Json(request.args),
        headers: Default::default(),
        invoke_key: app.invoke_key().to_owned(),
    };
    webview.on_message(
        invoke,
        Box::new(move |_, _, response, _, _| {
            reply.respond(response);
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listeners_are_admitted_only_for_events_the_main_webview_receives() {
        let reaches_main = |target: Value| {
            serde_json::from_value::<ListenArgs>(
                json!({"event":"sage:changed","target":target,"handler":3}),
            )
            .is_ok_and(|args| args.target.reaches_main())
        };
        assert!(reaches_main(json!({"kind":"Any"})));
        assert!(reaches_main(json!({"kind":"Window","label":"main"})));
        assert!(reaches_main(json!({"kind":"AnyLabel","label":"main"})));
        assert!(!reaches_main(
            json!({"kind":"AnyLabel","label":"browser-1"})
        ));
        assert!(!reaches_main(json!({"kind":"Window","label":"popup-1"})));
        assert!(!reaches_main(json!({"kind":"App"})));
    }
}
