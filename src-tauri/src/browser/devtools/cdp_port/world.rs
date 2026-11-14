//! Security item S8: every Agent script runs in an isolated world, never the
//! page's main world, so page code cannot replace the globals or prototypes
//! the script uses or read the values it handles. The page still sees an
//! Agent action's DOM effects and events, so it can tell something acted.

use serde::Deserialize;
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
const FRAME_TREE_BYTES: usize = 16 * 1_048_576;

#[derive(Clone, Default)]
pub(crate) struct AgentWorld(Arc<Mutex<WorldCache>>);

#[derive(Default)]
struct WorldCache {
    frame_id: Option<String>,
    target_info_refused: bool,
    reread_navigation: Option<u64>,
    context: Option<(u64, u64)>,
}

pub(super) fn left(deadline: Instant) -> Result<Duration, BrowserFailure> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(BrowserFailure::Unavailable);
    }
    Ok(left)
}

impl<T: CdpTransport> CdpPort<T> {
    /// `navigation` names the document by navigation id, so title or favicon
    /// changes keep the world. `None` means the page is loading and has no stamp:
    /// every step is a permitless read and the cached world is left as it is.
    pub(super) fn in_agent_world<R>(
        &self,
        navigation: Option<u64>,
        read_only: bool,
        deadline: Instant,
        script: impl Fn(u64, Duration) -> Result<R, BrowserFailure>,
    ) -> Result<R, BrowserFailure> {
        let read_only = read_only || navigation.is_none();
        let cached = self.cache()?.context;
        if navigation.is_none()
            && let Some((_, context)) = cached
            && self.context_alive(context, true, deadline)?
        {
            return Err(BrowserFailure::StaleHandle);
        }
        if let Some((cached_navigation, context)) = cached
            && navigation == Some(cached_navigation)
        {
            match script(context, left(deadline)?) {
                Err(BrowserFailure::Unavailable) => {
                    if self.context_alive(context, read_only, deadline)? {
                        return Err(BrowserFailure::Unavailable);
                    }
                }
                result => return result,
            }
        }
        let context = self.make_agent_world(navigation, read_only, deadline)?;
        script(context, left(deadline)?)
    }

    pub(super) fn evaluate_in_agent_world(
        &self,
        navigation: Option<u64>,
        expression: &str,
        max_result_bytes: usize,
        deadline: Instant,
    ) -> Result<Value, BrowserFailure> {
        let response = self.in_agent_world(navigation, true, deadline, |context, left| {
            let params =
                json!({"expression": expression, "contextId": context, "returnByValue": true});
            self.send(
                true,
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

    /// Issues one step. A `read_only` step goes without the dispatch permit:
    /// Chromium holds it while a navigation is pending, even one that raced
    /// the stamp, and a timed-out permitted call would revoke the whole grant.
    fn send(
        &self,
        read_only: bool,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        if read_only {
            self.transport
                .observe(method, params, max_result_bytes, timeout)
        } else {
            self.call(method, params, max_result_bytes, timeout)
        }
    }

    /// Whether `context` still answers; a failure there means its document
    /// went away, any other failure belongs to the script.
    fn context_alive(
        &self,
        context: u64,
        read_only: bool,
        deadline: Instant,
    ) -> Result<bool, BrowserFailure> {
        let params = json!({"expression": "0", "contextId": context});
        match self.send(
            read_only,
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

    /// The main frame's id equals the page target's id and outlives navigations,
    /// so it is read once per port.
    fn main_frame_id(
        &self,
        read_only: bool,
        deadline: Instant,
    ) -> Result<(String, bool), BrowserFailure> {
        if let Some(frame_id) = self.cache()?.frame_id.clone() {
            return Ok((frame_id, true));
        }
        let frame_id = self.read_frame_id(read_only, deadline)?;
        self.cache()?.frame_id = Some(frame_id.clone());
        Ok((frame_id, false))
    }

    fn read_frame_id(&self, read_only: bool, deadline: Instant) -> Result<String, BrowserFailure> {
        if !self.cache()?.target_info_refused {
            match self.send(
                read_only,
                "Target.getTargetInfo",
                "{}",
                TARGET_INFO_BYTES,
                left(deadline)?,
            ) {
                Ok(info) => {
                    return parse::<TargetInfoResult>(&info).map(|info| info.target_info.target_id);
                }
                Err(BrowserFailure::Unavailable) => self.cache()?.target_info_refused = true,
                Err(error) => return Err(error),
            }
        }
        let tree = self.send(
            read_only,
            "Page.getFrameTree",
            "{}",
            FRAME_TREE_BYTES,
            left(deadline)?,
        )?;
        parse::<FrameTreeResult>(&tree).map(|tree| tree.frame_tree.frame.id)
    }

    fn create_world(
        &self,
        frame_id: &str,
        read_only: bool,
        deadline: Instant,
    ) -> Result<u64, BrowserFailure> {
        let params = json!({"frameId": frame_id, "worldName": WORLD_NAME});
        let created = self.send(
            read_only,
            "Page.createIsolatedWorld",
            &params.to_string(),
            4096,
            left(deadline)?,
        )?;
        parse::<CreatedWorld>(&created).map(|created| created.execution_context_id)
    }

    fn make_agent_world(
        &self,
        navigation: Option<u64>,
        read_only: bool,
        deadline: Instant,
    ) -> Result<u64, BrowserFailure> {
        let (frame_id, cached) = self.main_frame_id(read_only, deadline)?;
        let context = match self.create_world(&frame_id, read_only, deadline) {
            // The page target may have been swapped, as a prerender
            // activation does; read its id again, once per navigation and
            // never while the page loads.
            Err(BrowserFailure::Unavailable)
                if cached && navigation.is_some() && self.claim_reread(navigation)? =>
            {
                let (frame_id, _) = self.main_frame_id(read_only, deadline)?;
                self.create_world(&frame_id, read_only, deadline)?
            }
            result => result?,
        };
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

    fn claim_reread(&self, navigation: Option<u64>) -> Result<bool, BrowserFailure> {
        let mut cache = self.cache()?;
        if cache.reread_navigation == navigation {
            return Ok(false);
        }
        cache.reread_navigation = navigation;
        cache.frame_id = None;
        Ok(true)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TargetInfoResult {
    target_info: TargetInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TargetInfo {
    target_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameTreeResult {
    frame_tree: FrameTree,
}

#[derive(Deserialize)]
struct FrameTree {
    frame: Frame,
}

#[derive(Deserialize)]
struct Frame {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatedWorld {
    execution_context_id: u64,
}

fn parse<R: serde::de::DeserializeOwned>(result: &str) -> Result<R, BrowserFailure> {
    serde_json::from_str(result).map_err(|_| BrowserFailure::Unavailable)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::acp::browser::WaitCondition;
    use crate::browser::devtools::{
        capability::DevtoolsPort, snapshot::DocumentStamp, transport::Settled,
    };

    #[derive(Default)]
    struct Page {
        stamp: Mutex<DocumentStamp>,
        frame_id: Mutex<&'static str>,
        live_context: Mutex<u64>,
        log: Mutex<Vec<(bool, String)>>,
        navigate_while_making_world: bool,
        refuse_target_info: bool,
        loading: AtomicBool,
        refuse_worlds: AtomicBool,
    }

    impl Page {
        fn new() -> Self {
            Self {
                frame_id: Mutex::new("main"),
                ..Self::default()
            }
        }

        fn calls(&self, method: &str) -> usize {
            let log = self.log.lock().unwrap();
            log.iter().filter(|(_, logged)| logged == method).count()
        }

        fn permitted(&self) -> usize {
            let log = self.log.lock().unwrap();
            log.iter().filter(|(permitted, _)| *permitted).count()
        }

        fn answer(&self, method: &str, params: &str) -> Result<String, BrowserFailure> {
            let params: Value = serde_json::from_str(params).unwrap();
            let frame_id = *self.frame_id.lock().unwrap();
            match method {
                "Target.getTargetInfo" if self.refuse_target_info => {
                    Err(BrowserFailure::Unavailable)
                }
                "Target.getTargetInfo" => {
                    Ok(json!({"targetInfo": {"targetId": frame_id}}).to_string())
                }
                "Page.getFrameTree" => {
                    Ok(json!({"frameTree": {"frame": {"id": frame_id}}}).to_string())
                }
                "Page.createIsolatedWorld" => {
                    if self.refuse_worlds.load(Ordering::SeqCst)
                        || params != json!({"frameId": frame_id, "worldName": WORLD_NAME})
                    {
                        return Err(BrowserFailure::Unavailable);
                    }
                    let mut live = self.live_context.lock().unwrap();
                    *live = self.calls(method) as u64 + 100;
                    if self.navigate_while_making_world {
                        self.stamp.lock().unwrap().navigation_id += 1;
                    }
                    Ok(json!({"executionContextId": *live}).to_string())
                }
                "Runtime.evaluate" if params["contextId"] == *self.live_context.lock().unwrap() => {
                    Ok(r#"{"result":{"value":true}}"#.into())
                }
                "Runtime.evaluate" => Err(BrowserFailure::Unavailable),
                _ => unreachable!("{method}"),
            }
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
            self.log.lock().unwrap().push((true, method.to_owned()));
            self.answer(method, params)
        }

        fn observe(
            &self,
            method: &str,
            params: &str,
            _max_result_bytes: usize,
            _timeout: Duration,
        ) -> Result<String, BrowserFailure> {
            self.log.lock().unwrap().push((false, method.to_owned()));
            self.answer(method, params)
        }

        fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
            if self.loading.load(Ordering::SeqCst) {
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
        let port = CdpPort::new(Page::new());
        assert_eq!(probe(&port), Ok(json!(true)));
        port.transport.stamp.lock().unwrap().generation += 1;
        assert_eq!(probe(&port), Ok(json!(true)));
        assert_eq!(worlds(&port), 1);
        *port.transport.live_context.lock().unwrap() = 0;
        assert_eq!(probe(&port), Ok(json!(true)));
        assert_eq!(worlds(&port), 2);
        port.transport.stamp.lock().unwrap().navigation_id += 1;
        assert_eq!(probe(&port), Ok(json!(true)));
        assert_eq!(worlds(&port), 3);
        assert_eq!(port.transport.calls("Target.getTargetInfo"), 1);
    }

    #[test]
    fn a_refused_target_info_falls_back_to_the_frame_tree_once_per_port() {
        let port = CdpPort::new(Page {
            refuse_target_info: true,
            ..Page::new()
        });
        assert_eq!(probe(&port), Ok(json!(true)));
        port.transport.stamp.lock().unwrap().navigation_id += 1;
        assert_eq!(probe(&port), Ok(json!(true)));
        assert_eq!(worlds(&port), 2);
        assert_eq!(port.transport.calls("Target.getTargetInfo"), 1);
        assert_eq!(port.transport.calls("Page.getFrameTree"), 1);
    }

    #[test]
    fn a_world_refused_for_a_cached_frame_id_reads_the_id_again_once() {
        let port = CdpPort::new(Page::new());
        assert_eq!(probe(&port), Ok(json!(true)));
        // The page target was swapped, as a prerender activation does.
        *port.transport.frame_id.lock().unwrap() = "swapped";
        port.transport.stamp.lock().unwrap().navigation_id += 1;
        assert_eq!(probe(&port), Ok(json!(true)));
        assert_eq!(worlds(&port), 3);
        assert_eq!(port.transport.calls("Target.getTargetInfo"), 2);
        port.transport.refuse_worlds.store(true, Ordering::SeqCst);
        port.transport.stamp.lock().unwrap().navigation_id += 1;
        assert_eq!(probe(&port), Err(BrowserFailure::Unavailable));
        assert_eq!(worlds(&port), 5);
        assert_eq!(port.transport.calls("Target.getTargetInfo"), 3);
        assert_eq!(probe(&port), Err(BrowserFailure::Unavailable));
        *port.transport.live_context.lock().unwrap() = 0;
        port.transport.loading.store(true, Ordering::SeqCst);
        let condition = WaitCondition::Text { text: "x".into() };
        let mut port = port;
        assert_eq!(port.matches(&condition, Duration::from_secs(1)), Ok(false));
        assert_eq!(worlds(&port), 7);
        assert_eq!(port.transport.calls("Target.getTargetInfo"), 3);
    }

    #[test]
    fn a_script_that_fails_in_a_live_world_is_not_run_again() {
        let port = CdpPort::new(Page::new());
        let runs = Mutex::new(0);
        let fail = |_: u64, _: Duration| -> Result<(), BrowserFailure> {
            *runs.lock().unwrap() += 1;
            Err(BrowserFailure::Unavailable)
        };
        let first = port.in_agent_world(Some(0), false, soon(), fail);
        let second = port.in_agent_world(Some(0), false, soon(), fail);
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
        let port = CdpPort::new(Page::new());
        let spent = Instant::now();
        assert_eq!(
            port.evaluate_in_agent_world(Some(0), "true", 4096, spent),
            Err(BrowserFailure::Unavailable)
        );
        assert!(port.transport.log.lock().unwrap().is_empty());
    }

    #[test]
    fn world_made_while_the_page_navigated_is_never_used_or_kept() {
        let port = CdpPort::new(Page {
            navigate_while_making_world: true,
            ..Page::new()
        });
        assert_eq!(probe(&port), Err(BrowserFailure::StaleHandle));
        assert_eq!(port.world.0.lock().unwrap().context, None);
    }

    #[test]
    fn text_wait_while_loading_probes_only_an_incoming_document() {
        let mut port = CdpPort::new(Page::new());
        let condition = WaitCondition::Text { text: "x".into() };
        assert_eq!(port.matches(&condition, Duration::from_secs(1)), Ok(true));
        let cached = port.world.0.lock().unwrap().context;
        port.transport.loading.store(true, Ordering::SeqCst);
        assert_eq!(port.matches(&condition, Duration::from_secs(1)), Ok(false));
        assert_eq!(worlds(&port), 1);
        *port.transport.live_context.lock().unwrap() = 0;
        assert_eq!(port.matches(&condition, Duration::from_secs(1)), Ok(true));
        assert_eq!(port.matches(&condition, Duration::from_secs(1)), Ok(true));
        assert_eq!(worlds(&port), 3);
        assert_eq!(port.world.0.lock().unwrap().context, cached);
    }

    #[test]
    fn wait_probes_and_page_context_reads_never_use_the_dispatch_permit() {
        let mut port = CdpPort::new(Page {
            refuse_target_info: true,
            ..Page::new()
        });
        let condition = WaitCondition::Text { text: "x".into() };
        let wait = |port: &mut CdpPort<Page>| port.matches(&condition, Duration::from_secs(1));
        assert_eq!(wait(&mut port), Ok(true));
        assert_eq!(wait(&mut port), Ok(true));
        assert_eq!(probe(&port), Ok(json!(true)));
        *port.transport.live_context.lock().unwrap() = 0;
        assert_eq!(wait(&mut port), Ok(true));
        *port.transport.frame_id.lock().unwrap() = "swapped";
        port.transport.stamp.lock().unwrap().navigation_id += 1;
        assert_eq!(wait(&mut port), Ok(true));
        port.transport.loading.store(true, Ordering::SeqCst);
        assert_eq!(wait(&mut port), Ok(false));
        *port.transport.live_context.lock().unwrap() = 0;
        assert_eq!(wait(&mut port), Ok(true));
        assert_eq!(worlds(&port), 5);
        assert_eq!(port.transport.calls("Page.getFrameTree"), 2);
        assert_eq!(port.transport.permitted(), 0);
        port.transport.loading.store(false, Ordering::SeqCst);
        port.transport.stamp.lock().unwrap().navigation_id += 1;
        let navigation = port.stamp().unwrap().navigation_id;
        let resolved = port.in_agent_world(Some(navigation), false, soon(), |_, _| Ok(()));
        assert_eq!(resolved, Ok(()));
        assert!(port.transport.permitted() > 0);
    }
}
