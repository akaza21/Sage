import React, { useState } from "react";
import { BrowserSurface } from "../../src/features/browser/BrowserSurface";
import { AgentArtifactTabStrip } from "../../src/workbench/AgentArtifactTabStrip";
import { AgentArtifactHeader } from "../../src/workbench/AgentArtifactHeader";
import { AgentArtifactFileHeaderBridge } from "../../src/workbench/useAgentArtifactHeaderSlot";
import { FileEditorHeader } from "../../src/features/files/FileEditorHeader";
import { FileMarkdownPreview } from "../../src/features/files/FileMarkdownPreview";
import { TerminalDock } from "../../src/workbench/TerminalDock";
import { terminalTheme } from "../../src/workbench/terminalTheme";
import { ChangesSurface } from "../../src/features/diff/ChangesSurface";
import { useWorkbenchStore } from "../../src/app/store";
import { acpProjectionStoreFor } from "../../src/app/acpProjectionRegistry";
import { pinAgentChatOwner } from "../../src/app/agentChatOwner";
import type { AcpChatPage, ProjectGeneration } from "../../src/domain/acp";
import type { ProjectSummary } from "../../src/domain/workbench";
import { DiffSurface } from "../../src/features/diff/DiffSurface";
import { openAgentArtifact } from "../../src/app/openAgentArtifact";
import type { AgentDiffRef } from "../../src/domain/agentDiff";
import type { WorkbenchTab } from "../../src/domain/workbench";
import "../../src/styles/global.css";
import "../../src/styles/shared-controls.css";
import "../../src/styles/file-editor-surface.css";
import "../../src/styles/preview-surface.css";
import "../../src/styles/agent-right-pane.css";
import "../../src/workbench/terminal-dock.css";

const mode = new URLSearchParams(location.search).get("surface");
type NativeSnapshotBridge = {
  gapCSnapshotBootstrap: () => Promise<{
    page: AcpChatPage;
    project: ProjectSummary;
    reference: AgentDiffRef;
  }>;
  gapCReadSnapshot: (request: unknown) => Promise<unknown>;
};
const nativeSnapshotBridge = window as unknown as NativeSnapshotBridge;
let historicalReference: AgentDiffRef | null = null;
let outputRead = false;
let callbackId = 0;
Object.assign(window, {
  __TAURI_INTERNALS__: {
    transformCallback: () => ++callbackId,
    unregisterCallback: () => undefined,
    invoke: async (command: string, args: Record<string, unknown>) => {
      if (command === "browser_hide")
        document
          .getElementById("native-result")
          ?.setAttribute("data-covered", String(args.cover === true));
      if (command === "browser_set_bounds")
        document
          .getElementById("native-result")
          ?.setAttribute("data-covered", "false");
      if (mode === "native-snapshot" && command === "get_acp_file_snapshot")
        return nativeSnapshotBridge.gapCReadSnapshot(args.request);
      if (command === "browser_status")
        return {
          tabId: "browser-test",
          url: mode === "new-tab" ? "" : "https://example.com/",
          title: "Example",
          revision: "1",
          loading: false,
          canGoBack: false,
          canGoForward: false,
          zoomPercent: 100,
        };
      if (command === "browser_preferences") return { showFullUrl: false };
      if (command === "browser_downloads")
        return [
          {
            tabId: "browser-test",
            id: 7,
            fileName: "report.pdf",
            source: "https://example.com/report.pdf",
            state: "downloading",
            receivedBytes: 5,
            totalBytes: 10,
          },
        ];
      if (
        command === "browser_suggested_sites" ||
        command === "browser_history"
      )
        return [];
      if (
        command === "browser_control" ||
        command === "browser_attachment_status"
      )
        return null;
      if (
        command === "browser_find" ||
        command === "browser_cancel_download" ||
        command === "terminal_write"
      )
        document.getElementById("native-result")!.textContent = JSON.stringify({
          command,
          ...args,
        });
      if (command === "terminal_read") {
        if (outputRead) return new Promise(() => undefined);
        outputRead = true;
        return {
          bytesBase64: btoa("SAVED OUTPUT\r\n"),
          nextCursor: "1",
          truncations: "0",
          truncated: false,
        };
      }
      return command === "plugin:event|listen" ? 1 : undefined;
    },
  },
});

const file: WorkbenchTab = {
  id: "file-test",
  kind: "file",
  title: "resource.md",
  resource: {
    rootId: "root",
    relativePath: "docs/long-segment/another-segment/work-items/resource.md",
  },
  position: 0,
  pinned: false,
  dirty: false,
};
const browser: WorkbenchTab = {
  ...file,
  id: "browser-test",
  kind: "browser",
  resource: undefined,
};

function FileFixture() {
  const [actions, setActions] = useState<HTMLDivElement | null>(null);
  const [details, setDetails] = useState<HTMLDivElement | null>(null);
  const [preview, setPreview] = useState(true);
  return (
    <>
      <AgentArtifactHeader
        tab={file}
        admitted
        page={null}
        position={0}
        total={1}
        onStep={() => undefined}
        onOpenInEditor={() => undefined}
        fileActionsRef={setActions}
        fileDetailsRef={setDetails}
      />
      <AgentArtifactFileHeaderBridge actions={actions} details={details}>
        <FileEditorHeader
          path={file.resource!.relativePath}
          lineEnding="lf"
          diskByteLen={2048}
          persisting={false}
          dirty={false}
          saving={false}
          transitioning={false}
          canPreview
          previewing={preview}
          agentMode
          agentMarkdown
          onTogglePreview={() => setPreview(!preview)}
        />
        {preview ? (
          <FileMarkdownPreview
            source={`# Resource\n\n\`\`\`ts\n${"long source ".repeat(150)}\n\`\`\``}
          />
        ) : (
          <pre>Live source</pre>
        )}
      </AgentArtifactFileHeaderBridge>
    </>
  );
}

function TabFixture() {
  const [tabs, setTabs] = useState(
    Array.from({ length: 18 }, (_, index) => ({
      ...file,
      id: `tab-${index}`,
      title: `file-${index}.ts`,
      resource: { rootId: "root", relativePath: `src/file-${index}.ts` },
    })),
  );
  const [active, setActive] = useState("tab-0");
  return (
    <>
      <button onClick={() => setActive("tab-17")}>
        Select last externally
      </button>
      <AgentArtifactTabStrip
        artifacts={tabs}
        selectedTarget={{ kind: "tab", tabId: active }}
        focusedOwner={null}
        focusedPage={null}
        previewSelected={false}
        previewOpen={false}
        subagentOpen={false}
        subagentSelected={false}
        onSelectSubagent={() => undefined}
        record={null}
        availableTargets={[]}
        activateTab={setActive}
        closeTab={(id) =>
          setTabs((current) => current.filter((tab) => tab.id !== id))
        }
        panelId="resource-panel"
      />
      <div id="resource-panel" role="tabpanel">
        {active}
      </div>
    </>
  );
}

function TerminalFixture() {
  const [status, setStatus] = useState("");
  const [scope] = useState({
    projectId: "project",
    projectGeneration: "0",
    chatId: "chat",
  });
  const [session] = useState({
    sessionId: "f99cd56a-32d9-4600-a1f6-269e05734268",
    revision: "1",
    shellPath: "cmd.exe",
  });
  return (
    <div
      style={{
        height: "300px",
        display: "grid",
        gridTemplateRows: "minmax(0, 1fr) auto",
      }}
    >
      <TerminalDock
        getTheme={terminalTheme}
        scope={scope}
        session={session}
        ended={false}
        onStatus={setStatus}
      />
      <p>{status}</p>
    </div>
  );
}

function ChangesFixture() {
  useWorkbenchStore.setState({
    repositories: {
      roots: [
        {
          rootId: "root",
          state: "repository",
          branch: "main",
          elapsedMs: 0,
          truncated: false,
          message: null,
          changes: [],
        },
      ],
      elapsedMs: 0,
      truncated: false,
    },
  });
  return <ChangesSurface />;
}

function NativeSnapshotFixture() {
  const tab = useWorkbenchStore((state) =>
    state.tabs.find((item) => item.id === state.activeTabId),
  );
  const page = acpProjectionStoreFor(
    useWorkbenchStore.getState().tabs.find((item) => item.kind === "chat")!.id,
  ).getState().page;
  return tab?.kind === "diff" ? (
    <DiffSurface tab={tab} readOnly />
  ) : (
    <>
      <button
        type="button"
        onClick={() => {
          const ref = historicalReference;
          if (ref)
            void openAgentArtifact({
              owner: ref,
              target: "diff",
              resource: ref,
              diffRef: ref,
            });
        }}
      >
        Open retained saved diff
      </button>
      <ChangesSurface page={page} />
    </>
  );
}

if (mode === "native-snapshot") {
  const { page, project, reference } =
    await nativeSnapshotBridge.gapCSnapshotBootstrap();
  historicalReference = reference;
  const scope = {
    projectId: page.projectId,
    chatId: page.chatId,
    projectGeneration: "0" as ProjectGeneration,
  };
  const projection = acpProjectionStoreFor(page.chatId);
  projection.getState().activateScope(scope);
  projection.getState().applyAuthoritativePage(scope, page);
  useWorkbenchStore.setState({
    project,
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,
    tabs: [
      {
        id: page.chatId,
        kind: "chat",
        title: page.title,
        pinned: false,
        dirty: false,
        position: 0,
      },
    ],
    activeTabId: page.chatId,
  });
  pinAgentChatOwner(page.chatId);
}

export function GapCFixture() {
  return (
    <>
      <main
        className="workbench workbench--agent"
        style={{
          display: "block",
          width: "min(100vw, 560px)",
          height: "min(100vh, 400px)",
          position: "relative",
          overflow: "hidden",
        }}
      >
        {mode === "native-snapshot" ? (
          <NativeSnapshotFixture />
        ) : mode === "file" ? (
          <FileFixture />
        ) : mode === "tabs" ? (
          <TabFixture />
        ) : mode === "terminal" ? (
          <TerminalFixture />
        ) : mode === "changes" ? (
          <ChangesFixture />
        ) : (
          <BrowserSurface tab={browser} />
        )}
      </main>
      <output id="native-result" />
    </>
  );
}
