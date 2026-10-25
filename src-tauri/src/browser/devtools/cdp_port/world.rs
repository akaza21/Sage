//! Security item S8: every Agent script runs in an isolated world, never the
//! page's main world, so page code cannot replace the globals or prototypes the
//! script uses or read the values it handles. The page still sees the DOM
//! effects and events of an Agent action, so it can tell that something acted.

use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::acp::browser::BrowserFailure;

use super::{CdpPort, CdpTransport};

const WORLD_NAME: &str = "sage-agent";
/// One target's info: a URL of at most 2 MiB and a title of at most 4,096
/// characters, as Chromium caps them, with room for JSON escapes.
const TARGET_INFO_BYTES: usize = 3 * 1_048_576;

/// The main frame's id and the isolated world's execution context for the
/// last navigation it was made in, shared by every clone of a port.
#[derive(Clone, Default)]
pub(crate) struct AgentWorld(Arc<Mutex<WorldCache>>);

#[derive(Default)]
struct WorldCache {
    frame_id: Option<String>,
    context: Option<(u64, u64)>,
}

/// The time left before `deadline`; a spent budget issues nothing more.
pub(super) fn left(deadline: Instant) -> Result<Duration, BrowserFailure> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(BrowserFailure::Unavailable);
    }
    Ok(left)
}

impl<T: CdpTransport> CdpPort<T> {
    /// Runs `script` with the isolated world's context id and the time left.
    /// `navigation` names the document by navigation id, so title or favicon
    /// changes keep the world; the world is made once per navigation and again
    /// when its cached context is gone. `None` means the page is loading and
    /// has no stamp: the world is made for the document the frame shows and
    /// is not kept.
    pub(super) fn in_agent_world<R>(
        &self,
        navigation: Option<u64>,
        deadline: Instant,
        script: impl Fn(u64, Duration) -> Result<R, BrowserFailure>,
    ) -> Result<R, BrowserFailure> {
        let cached = self.cache()?.context;
        if let Some((cached_navigation, context)) = cached
            && navigation == Some(cached_navigation)
        {
            match script(context, left(deadline)?) {
                Err(BrowserFailure::Unavailable) => {
                    if self.context_alive(context, deadline)? {
                        return Err(BrowserFailure::Unavailable);
                    }
                }
                result => return result,
            }
        }
        let context = self.make_agent_world(navigation, deadline)?;
        script(context, left(deadline)?)
    }

    /// Evaluates `expression` in the isolated world and returns its value.
    pub(super) fn evaluate_in_agent_world(
        &self,
        navigation: Option<u64>,
        expression: &str,
        max_result_bytes: usize,
        deadline: Instant,
    ) -> Result<Value, BrowserFailure> {
        let response = self.in_agent_world(navigation, deadline, |context, left| {
            let params =
                json!({"expression": expression, "contextId": context, "returnByValue": true});
            self.call(
                "Runtime.evaluate",
                &params.to_string(),
                max_result_bytes,
                left,
            )
        })?;
        let response: Value =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        if response.get("exceptionDetails").is_some() {
            return Err(BrowserFailure::Unavailable);
        }
        Ok(response["result"]["value"].clone())
    }

    fn cache(&self) -> Result<std::sync::MutexGuard<'_, WorldCache>, BrowserFailure> {
        self.world.0.lock().map_err(|_| BrowserFailure::Unavailable)
    }

    /// Whether `context` still answers; a failure there means its document
    /// went away, any other failure belongs to the script.
    fn context_alive(&self, context: u64, deadline: Instant) -> Result<bool, BrowserFailure> {
        let params = json!({"expression": "0", "contextId": context});
        match self.call(
            "Runtime.evaluate",
            &params.to_string(),
            4096,
            left(deadline)?,
        ) {
            Ok(_) => Ok(true),
            Err(BrowserFailure::Unavailable) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// The main frame's id. It equals the page target's id and outlives
    /// navigations, so it is read once per port, from the bounded target info
    /// rather than the page-sized frame tree.
    fn main_frame_id(&self, deadline: Instant) -> Result<String, BrowserFailure> {
        if let Some(frame_id) = self.cache()?.frame_id.clone() {
            return Ok(frame_id);
        }
        let info = self.call(
            "Target.getTargetInfo",
            "{}",
            TARGET_INFO_BYTES,
            left(deadline)?,
        )?;
        let info: Value = serde_json::from_str(&info).map_err(|_| BrowserFailure::Unavailable)?;
        let frame_id = info["targetInfo"]["targetId"]
            .as_str()
            .ok_or(BrowserFailure::Unavailable)?
            .to_owned();
        self.cache()?.frame_id = Some(frame_id.clone());
        Ok(frame_id)
    }

    fn make_agent_world(
        &self,
        navigation: Option<u64>,
        deadline: Instant,
    ) -> Result<u64, BrowserFailure> {
        let params = json!({"frameId": self.main_frame_id(deadline)?, "worldName": WORLD_NAME});
        let created = self.call(
            "Page.createIsolatedWorld",
            &params.to_string(),
            4096,
            left(deadline)?,
        )?;
        let created: Value =
            serde_json::from_str(&created).map_err(|_| BrowserFailure::Unavailable)?;
        let context = created["executionContextId"]
            .as_u64()
            .ok_or(BrowserFailure::Unavailable)?;
        // The world belongs to whichever document the frame showed; keep it
        // only when that is still the navigation the caller acts on.
        if let Some(navigation) = navigation {
            if self.stamp()?.navigation_id != navigation {
                return Err(BrowserFailure::StaleHandle);
            }
            self.cache()?.context = Some((navigation, context));
        }
        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::acp::browser::WaitCondition;
    use crate::browser::devtools::{
        capability::DevtoolsPort, snapshot::DocumentStamp, transport::Settled,
    };

    /// A page whose isolated world answers only while its context is live.
    #[derive(Default)]
    struct Page {
        stamp: Mutex<DocumentStamp>,
        live_context: Mutex<u64>,
        calls: Mutex<HashMap<String, usize>>,
        navigate_while_making_world: bool,
        loading: bool,
    }

    impl Page {
        fn calls(&self, method: &str) -> usize {
            self.calls.lock().unwrap().get(method).copied().unwrap_or(0)
        }
    }

    impl CdpTransport for Page {
        fn call(
            &self,
            method: &str,
            params: &str,
            _max_result_bytes: usize,
            _timeout: Duration,
        ) -> Result<String, BrowserFailure> {
            *self
                .calls
                .lock()
                .unwrap()
                .entry(method.to_owned())
                .or_default() += 1;
            let params: Value = serde_json::from_str(params).unwrap();
            match method {
                "Target.getTargetInfo" => Ok(r#"{"targetInfo":{"targetId":"main"}}"#.into()),
                "Page.createIsolatedWorld" => {
                    assert_eq!(params, json!({"frameId": "main", "worldName": WORLD_NAME}));
                    let context = self.calls(method) as u64;
                    *self.live_context.lock().unwrap() = context;
                    if self.navigate_while_making_world {
                        self.stamp.lock().unwrap().navigation_id += 1;
                    }
                    Ok(json!({"executionContextId": context}).to_string())
                }
                "Runtime.evaluate" if params["contextId"] == *self.live_context.lock().unwrap() => {
                    Ok(r#"{"result":{"value":true}}"#.into())
                }
                "Runtime.evaluate" => Err(BrowserFailure::Unavailable),
                _ => unreachable!("{method}"),
            }
        }

        fn observe(
            &self,
            method: &str,
            _: &str,
            _: usize,
            _: Duration,
        ) -> Result<String, BrowserFailure> {
            unreachable!("{method}")
        }

        fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
            if self.loading {
                return Err(BrowserFailure::Unavailable);
            }
            Ok(*self.stamp.lock().unwrap())
        }

        fn issued(&self) -> bool {
            true
        }

        fn settled(&self, _: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
            unreachable!()
        }
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    fn probe(port: &CdpPort<Page>) -> Result<Value, BrowserFailure> {
        let navigation = port.stamp()?.navigation_id;
        port.evaluate_in_agent_world(Some(navigation), "true", 4096, soon())
    }

    fn worlds(port: &CdpPort<Page>) -> usize {
        port.transport.calls("Page.createIsolatedWorld")
    }

    #[test]
    fn world_is_made_once_per_navigation_and_again_when_its_context_dies() {
        let port = CdpPort::new(Page::default());
        assert_eq!(probe(&port), Ok(json!(true)));
        // A title or favicon change moves only the generation.
        port.transport.stamp.lock().unwrap().generation += 1;
        assert_eq!(probe(&port), Ok(json!(true)));
        assert_eq!(worlds(&port), 1);
        // The document went away before its stamp moved.
        *port.transport.live_context.lock().unwrap() = 0;
        assert_eq!(probe(&port), Ok(json!(true)));
        assert_eq!(worlds(&port), 2);
        port.transport.stamp.lock().unwrap().navigation_id += 1;
        assert_eq!(probe(&port), Ok(json!(true)));
        assert_eq!(worlds(&port), 3);
        assert_eq!(port.transport.calls("Target.getTargetInfo"), 1);
    }

    #[test]
    fn a_script_that_fails_in_a_live_world_is_not_run_again() {
        let port = CdpPort::new(Page::default());
        let runs = Mutex::new(0);
        let fail = |_: u64, _: Duration| -> Result<(), BrowserFailure> {
            *runs.lock().unwrap() += 1;
            Err(BrowserFailure::Unavailable)
        };
        let first = port.in_agent_world(Some(0), soon(), fail);
        let second = port.in_agent_world(Some(0), soon(), fail);
        assert_eq!(
            (first, second),
            (
                Err(BrowserFailure::Unavailable),
                Err(BrowserFailure::Unavailable)
            )
        );
        assert_eq!(*runs.lock().unwrap(), 2);
        assert_eq!(worlds(&port), 1);
    }

    #[test]
    fn a_spent_budget_issues_nothing() {
        let port = CdpPort::new(Page::default());
        let spent = Instant::now();
        assert_eq!(
            port.evaluate_in_agent_world(Some(0), "true", 4096, spent),
            Err(BrowserFailure::Unavailable)
        );
        assert!(port.transport.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn world_made_while_the_page_navigated_is_never_used_or_kept() {
        let port = CdpPort::new(Page {
            navigate_while_making_world: true,
            ..Page::default()
        });
        assert_eq!(probe(&port), Err(BrowserFailure::StaleHandle));
        assert_eq!(port.world.0.lock().unwrap().context, None);
    }

    #[test]
    fn text_wait_probes_the_isolated_world_while_the_page_is_loading() {
        let mut port = CdpPort::new(Page {
            loading: true,
            ..Page::default()
        });
        let condition = WaitCondition::Text { text: "x".into() };
        assert_eq!(port.matches(&condition, Duration::from_secs(1)), Ok(true));
        assert_eq!(port.matches(&condition, Duration::from_secs(1)), Ok(true));
        assert_eq!(worlds(&port), 2);
        assert_eq!(port.world.0.lock().unwrap().context, None);
    }
}
