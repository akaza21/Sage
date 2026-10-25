//! Security item S8: every Agent script runs in an isolated world, never the
//! page's main world, so page code cannot replace the globals the script uses,
//! read what it touches or notice it ran.

use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::acp::browser::BrowserFailure;
use crate::browser::devtools::snapshot::DocumentStamp;

use super::{CdpPort, CdpTransport};

const WORLD_NAME: &str = "sage-agent";

/// The isolated world's execution context for the last document it was made
/// in, shared by every clone of a port.
#[derive(Clone, Default)]
pub(crate) struct AgentWorld(Arc<Mutex<Option<(DocumentStamp, u64)>>>);

impl<T: CdpTransport> CdpPort<T> {
    /// Runs `script` with the isolated world's context id for the document
    /// `stamp` names. The world is made once per document; a cached context
    /// that fails is made again once, in case its document went away.
    pub(super) fn in_agent_world<R>(
        &self,
        stamp: DocumentStamp,
        timeout: Duration,
        script: impl Fn(u64) -> Result<R, BrowserFailure>,
    ) -> Result<R, BrowserFailure> {
        let cached = *self
            .world
            .0
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?;
        if let Some((cached_stamp, context)) = cached
            && cached_stamp == stamp
        {
            match script(context) {
                Err(BrowserFailure::Unavailable) => {}
                result => return result,
            }
        }
        script(self.make_agent_world(stamp, timeout)?)
    }

    /// Evaluates `expression` in the isolated world and returns its value.
    pub(super) fn evaluate_in_agent_world(
        &self,
        stamp: DocumentStamp,
        expression: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<Value, BrowserFailure> {
        let response = self.in_agent_world(stamp, timeout, |context| {
            let params =
                json!({"expression": expression, "contextId": context, "returnByValue": true});
            self.call(
                "Runtime.evaluate",
                &params.to_string(),
                max_result_bytes,
                timeout,
            )
        })?;
        let response: Value =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        if response.get("exceptionDetails").is_some() {
            return Err(BrowserFailure::Unavailable);
        }
        Ok(response["result"]["value"].clone())
    }

    fn make_agent_world(
        &self,
        stamp: DocumentStamp,
        timeout: Duration,
    ) -> Result<u64, BrowserFailure> {
        let tree = self.call("Page.getFrameTree", "{}", 65_536, timeout)?;
        let tree: Value = serde_json::from_str(&tree).map_err(|_| BrowserFailure::Unavailable)?;
        let frame_id = tree["frameTree"]["frame"]["id"]
            .as_str()
            .ok_or(BrowserFailure::Unavailable)?;
        let params = json!({"frameId": frame_id, "worldName": WORLD_NAME});
        let created = self.call(
            "Page.createIsolatedWorld",
            &params.to_string(),
            4096,
            timeout,
        )?;
        let created: Value =
            serde_json::from_str(&created).map_err(|_| BrowserFailure::Unavailable)?;
        let context = created["executionContextId"]
            .as_u64()
            .ok_or(BrowserFailure::Unavailable)?;
        // The world belongs to whichever document the frame showed; keep it
        // only when that is still the document the caller acts on.
        if self.stamp()? != stamp {
            return Err(BrowserFailure::StaleHandle);
        }
        *self
            .world
            .0
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)? = Some((stamp, context));
        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::devtools::transport::Settled;

    /// A page whose isolated world answers only while its context is live.
    #[derive(Default)]
    struct Page {
        stamp: Mutex<u64>,
        live_context: Mutex<u64>,
        worlds: Mutex<u64>,
        navigate_while_making_world: bool,
    }

    impl CdpTransport for Page {
        fn call(
            &self,
            method: &str,
            params: &str,
            _max_result_bytes: usize,
            _timeout: Duration,
        ) -> Result<String, BrowserFailure> {
            let params: Value = serde_json::from_str(params).unwrap();
            match method {
                "Page.getFrameTree" => Ok(r#"{"frameTree":{"frame":{"id":"main"}}}"#.into()),
                "Page.createIsolatedWorld" => {
                    assert_eq!(params, json!({"frameId": "main", "worldName": WORLD_NAME}));
                    let mut worlds = self.worlds.lock().unwrap();
                    *worlds += 1;
                    *self.live_context.lock().unwrap() = *worlds;
                    if self.navigate_while_making_world {
                        *self.stamp.lock().unwrap() += 1;
                    }
                    Ok(json!({"executionContextId": *worlds}).to_string())
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
            Ok(document(*self.stamp.lock().unwrap()))
        }

        fn issued(&self) -> bool {
            true
        }

        fn settled(&self, _: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
            unreachable!()
        }
    }

    fn document(id: u64) -> DocumentStamp {
        DocumentStamp {
            navigation_id: id,
            generation: id,
        }
    }

    fn probe(port: &CdpPort<Page>, id: u64) -> Result<Value, BrowserFailure> {
        port.evaluate_in_agent_world(document(id), "true", 4096, Duration::from_secs(1))
    }

    #[test]
    fn world_is_made_once_per_document_and_again_when_its_context_dies() {
        let port = CdpPort::new(Page::default());
        assert_eq!(probe(&port, 0), Ok(json!(true)));
        assert_eq!(probe(&port, 0), Ok(json!(true)));
        assert_eq!(*port.transport.worlds.lock().unwrap(), 1);
        // The document went away before its stamp moved.
        *port.transport.live_context.lock().unwrap() = 0;
        assert_eq!(probe(&port, 0), Ok(json!(true)));
        assert_eq!(*port.transport.worlds.lock().unwrap(), 2);
        *port.transport.stamp.lock().unwrap() = 1;
        assert_eq!(probe(&port, 1), Ok(json!(true)));
        assert_eq!(*port.transport.worlds.lock().unwrap(), 3);
    }

    #[test]
    fn world_made_while_the_page_navigated_is_never_used_or_kept() {
        let port = CdpPort::new(Page {
            navigate_while_making_world: true,
            ..Page::default()
        });
        assert_eq!(probe(&port, 0), Err(BrowserFailure::StaleHandle));
        assert_eq!(*port.world.0.lock().unwrap(), None);
    }
}
