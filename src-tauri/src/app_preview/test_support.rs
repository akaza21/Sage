//! Real App Preview children for native tests outside this module.
//!
//! Project-transition commands own the epoch bumps that must retire Preview
//! children, but they cannot reach the lifecycle registry directly. This
//! fixture starts a genuine `npm run dev` HTTP server through the production
//! lifecycle so those tests can observe whether the process really stopped.

use std::{fs, net::TcpStream, path::PathBuf};

use super::{
    discovery::{PreviewDiscoveryRequest, PreviewScriptName, PreviewTargetRef},
    lifecycle,
};
use sage_acp_protocol::contract::{
    ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageRootId,
};

const SERVER: &str = r#"const http = require("http");
const server = http.createServer((_, response) => response.end());
server.listen(0, "127.0.0.1", () => console.log(`http://127.0.0.1:${server.address().port}`));
"#;

/// A Preview child admitted for one Project-transition epoch.
///
/// The registry is per test thread, so the transition under test must run on
/// the thread that started the child (for example inside `Runtime::block_on`).
pub(crate) struct ServingPreview {
    root: PathBuf,
    target: PreviewTargetRef,
    port: u16,
}

impl ServingPreview {
    /// Starts a real HTTP server bound to `transition_epoch` and returns once
    /// the production lifecycle has observed its loopback origin.
    pub(crate) fn start(transition_epoch: u64) -> Self {
        let root =
            std::env::temp_dir().join(format!("sage-preview-epoch-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"dev":"node server.js"}}"#,
        )
        .unwrap();
        fs::write(root.join("server.js"), SERVER).unwrap();
        let target = PreviewTargetRef {
            project_id: SageProjectId::parse(format!("project-preview-{}", uuid::Uuid::new_v4()))
                .unwrap(),
            project_generation: ProjectGeneration::new(transition_epoch).unwrap(),
            expected_transition_epoch: transition_epoch,
            chat_id: SageChatId::parse("chat-preview").unwrap(),
            session_id: SageAgentSessionId::parse("session-preview").unwrap(),
            runtime_epoch: RuntimeEpoch::new(3).unwrap(),
            root_id: SageRootId::parse("root-preview").unwrap(),
            script_name: PreviewScriptName::Dev,
        };
        let request = PreviewDiscoveryRequest {
            project_id: target.project_id.clone(),
            project_generation: target.project_generation,
            expected_transition_epoch: transition_epoch,
            chat_id: target.chat_id.clone(),
            session_id: target.session_id.clone(),
            runtime_epoch: target.runtime_epoch,
        };
        lifecycle::replace_discovered(&request, vec![(target.clone(), root.clone())]);
        lifecycle::begin_start(&target, &|_| Ok(())).unwrap();
        let reservation = lifecycle::reserve_start(target.clone()).unwrap();
        lifecycle::mark_waiting_for_origin(&target, &|_| Ok(())).unwrap();
        let started = lifecycle::finish_start(reservation, &|_| Ok(())).unwrap();
        let port = started
            .origin
            .rsplit(':')
            .next()
            .and_then(|port| port.trim_end_matches('/').parse().ok())
            .expect("Preview origin carries a loopback port");
        Self { root, target, port }
    }

    /// Whether the child still accepts connections and the registry still
    /// owns it. Cleanup proves tree-zero and joins the output drainers before
    /// it returns, so a stopped child has already closed its listener.
    pub(crate) fn is_serving(&self) -> bool {
        let owned = lifecycle::test_owned_child_count() == 1;
        let listening = TcpStream::connect(("127.0.0.1", self.port)).is_ok();
        assert_eq!(
            owned, listening,
            "registry ownership and the child's listener must agree"
        );
        listening
    }
}

impl Drop for ServingPreview {
    fn drop(&mut self) {
        if lifecycle::test_owned_child_count() != 0 {
            let _ = lifecycle::stop(&self.target);
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
