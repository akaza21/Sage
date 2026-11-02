import { createRoot } from "react-dom/client";
import { useState } from "react";
import { AgentArtifactTabStrip } from "../../../src/workbench/AgentArtifactTabStrip";
import { AgentArtifactPaneControls } from "../../../src/workbench/AgentArtifactPaneControls";
import type { AgentArtifactTarget } from "../../../src/app/agentArtifactFollowStore";
import "../../../src/styles/global.css";
import "../../../src/styles/workbench-chrome.css";
import "../../../src/workbench/workbench-window-mode.css";
import "../../../src/styles/agent-right-pane.css";

const artifacts = Array.from({ length: 8 }, (_, position) => ({
  id: `browser-${position}`,
  title: `Browser page ${position + 1}`,
  kind: "browser" as const,
  position,
  pinned: false,
  dirty: false,
}));
function Fixture() {
  const [selected, select] = useState<AgentArtifactTarget>({
    kind: "tab",
    tabId: "browser-0",
  });
  return (
    <div className="workbench--agent">
      <button onClick={() => select({ kind: "tab", tabId: "browser-7" })}>
        Select last resource
      </button>
      <aside
        className="agent-artifact-pane"
        style={{ width: "min(560px, 100vw)", height: 400 }}
      >
        <header className="agent-artifact-pane__header">
          <AgentArtifactTabStrip
            artifacts={artifacts}
            selectedTarget={selected}
            focusedOwner={null}
            focusedPage={null}
            previewOpen={false}
            previewSelected={false}
            subagentOpen={false}
            subagentSelected={false}
            onSelectSubagent={() => {}}
            record={null}
            availableTargets={[]}
            activateTab={(tabId) => select({ kind: "tab", tabId })}
            closeTab={() => {}}
          />
          <AgentArtifactPaneControls
            maximized={false}
            onMaximizedChange={() => {}}
            disabled
            onNewTab={() => {}}
            onCollapse={() => {}}
          />
        </header>
      </aside>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
