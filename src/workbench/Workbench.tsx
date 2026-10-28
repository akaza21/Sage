import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { useWorkbenchStore } from "../app/store";
import { keymapBinding, matchesPlatformChord } from "../domain/keymapRegistry";
import { usesMetaModifier } from "../ui/shortcutLabel";
import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { useSessionAccessModeStore } from "../app/sessionAccessModeStore";
import { useUiAppearanceStore } from "../app/uiAppearanceStore";
import { admitsFocusedCodexRendererRecreation } from "../features/chat/acp/rendererRecreationCollectorAdmission";
import { mountRendererRecreationPacketCollector } from "../features/chat/acp/rendererRecreationPacket";
import { ActiveEditorCanvas } from "./ActiveEditorCanvas";
import { DirtyTabDialog } from "./DirtyTabDialog";
import { RightSidebar } from "./RightSidebar";
import { StatusBar } from "./StatusBar";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import { TabStrip } from "./TabStrip";
import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";
import { TitleBar } from "./TitleBar";
import {
  type AgentCenterDecision,
  agentCenterChatAction,
  DEFAULT_WORKBENCH_WINDOW_MODE,
  EDITOR_CANVAS_KINDS,
  isAgentArtifactTabKind,
  isCompactDrawerOpen,
  lastArtifactTabId,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";
import {
  EDITOR_ACTIVITY_BAR_WIDTH,
  EDITOR_TOOL_RAIL_WIDTH,
} from "./workbenchSidebarResize";
import {
  readWorkbenchLayout,
  writeWorkbenchLayout,
} from "./workbenchLayoutStorage";
import "./workbench-window-mode.css";
import "./harness-browser-mode.css";
import { useAgentDesktopSidebarWidths } from "./useAgentDesktopSidebarWidths";
import { useCompactWorkbenchLayout } from "./useCompactWorkbenchLayout";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { pinnedAgentChatOwner } from "../app/agentChatOwner";

registerWorkbenchSurfaces();

const loadAgentWorkbenchBody = () =>
  import("./AgentWorkbenchBody").then(({ AgentWorkbenchBody }) => ({
    default: AgentWorkbenchBody,
  }));

const loadTerminalDock = () =>
  import("./TerminalDockShell").then(({ TerminalDockForWorkbench }) => ({
    default: TerminalDockForWorkbench,
  }));

const workbenchGeometryStyle = {
  "--editor-activity-width": `${EDITOR_ACTIVITY_BAR_WIDTH}px`,
} as CSSProperties;

function EditorContinuityKeeper({
  windowMode,
}: {
  windowMode: WorkbenchWindowMode;
}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const lastArtifactId = useRef<string | null>(null);
  const previousMode = useRef<WorkbenchWindowMode | null>(null);

  useEffect(() => {
    const active = tabs.find((tab) => tab.id === activeTabId);
    if (active && isAgentArtifactTabKind(active.kind)) {
      lastArtifactId.current = active.id;
    }
  }, [activeTabId, tabs]);

  useEffect(() => {
    const enteredEditor =
      windowMode === "editor" && previousMode.current !== "editor";
    previousMode.current = windowMode;
    if (!enteredEditor) return;
    const state = useWorkbenchStore.getState();
    const active =
      state.tabs.find((tab) => tab.id === state.activeTabId) ?? null;
    if (active && isAgentArtifactTabKind(active.kind)) return;
    const continuity = lastArtifactTabId(state.tabs, lastArtifactId.current);
    if (continuity && continuity !== state.activeTabId) {
      void state.activateTab(continuity);
    }
  }, [windowMode]);

  return null;
}

function agentCenterAttemptKey(
  decision: AgentCenterDecision,
  projectId: string | null,
): string | null {
  const scope = projectId ?? "";
  if (decision.action === "create") return `create:${scope}`;
  return null;
}

function AgentCenterKeeper({
  windowMode,
}: {
  windowMode: WorkbenchWindowMode;
}) {
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const attemptedRef = useRef<string | null>(null);

  useEffect(() => {
    if (windowMode !== "agent") {
      attemptedRef.current = null;
      return;
    }
    if (projectTransitioning) return;
    const state = useWorkbenchStore.getState();
    const decision = agentCenterChatAction(state.tabs, activeTabId);
    if (decision.action === "none") {
      attemptedRef.current = null;
      return;
    }
    const attemptKey = agentCenterAttemptKey(
      decision,
      state.project?.id ?? null,
    );
    if (attemptKey === null || attemptedRef.current === attemptKey) return;
    attemptedRef.current = attemptKey;
    void state.createChat();
  }, [windowMode, activeTabId, projectTransitioning, projectGeneration]);

  return null;
}

function RendererRecreationPacketCollector({
  windowMode,
}: {
  windowMode: WorkbenchWindowMode;
}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const selectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const activeTab = tabs.find((tab) => tab.id === activeTabId);
  const admitted = admitsFocusedCodexRendererRecreation({
    windowMode,
    activeTabId,
    focusedChatId: activeTab?.kind === "chat" ? activeTab.id : null,
    harness: harnesses.find((harness) => harness.id === selectedHarnessId),
  });

  useEffect(() => {
    if (!admitted) return;
    return mountRendererRecreationPacketCollector();
  }, [admitted]);

  return null;
}

export function Workbench() {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const [terminalOpen, setTerminalOpen] = useState(false);
  const compact = useCompactWorkbenchLayout();
  const [initialLayout] = useState(readWorkbenchLayout);
  const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>(
    initialLayout.windowMode ?? DEFAULT_WORKBENCH_WINDOW_MODE,
  );
  const [agentArtifactExpanded, setAgentArtifactExpanded] = useState(false);
  const [agentNavigationCollapsed, setAgentNavigationCollapsed] =
    useState(false);
  const { changeWidth: changeAgentSidebarWidth, layout: agentLayout } =
    useAgentDesktopSidebarWidths(
      agentArtifactExpanded,
      agentNavigationCollapsed,
    );
  const resourceModal = windowMode === "agent" && agentLayout.artifactModal;
  const [editorToolRailWidth, setEditorToolRailWidth] = useState(
    initialLayout.editorToolRailWidth,
  );
  useEffect(() => {
    void useEditorSettingsStore.getState().restore();
    void useSessionAccessModeStore.getState().restore();
    void useUiAppearanceStore.getState().restore();
  }, []);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (resourceModal) return;
      // Platform modifier only: Control+N on macOS is the editor's next line.
      const apple = usesMetaModifier();
      if (matchesPlatformChord(keymapBinding("openSettings"), event, apple)) {
        event.preventDefault();
        setWindowMode("editor");
        void useWorkbenchStore.getState().openSettings();
      }
      if (matchesPlatformChord(keymapBinding("newTask"), event, apple)) {
        event.preventDefault();
        setWindowMode("agent");
        void useWorkbenchStore.getState().createChat();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [resourceModal]);
  useEffect(() => {
    writeWorkbenchLayout({
      editorToolRailWidth,
      windowMode,
    });
  }, [editorToolRailWidth, windowMode]);
  const changeAgentArtifact = useCallback((expanded: boolean) => {
    setAgentArtifactExpanded(expanded);
  }, []);
  const openArtifactInEditor = useCallback((tabId: string) => {
    void (async () => {
      const before = useWorkbenchStore.getState();
      if (
        before.projectTransitioning ||
        !before.tabs.some(
          (tab) => tab.id === tabId && isAgentArtifactTabKind(tab.kind),
        )
      )
        return;
      await before.activateTab(tabId);
      const after = useWorkbenchStore.getState();
      if (
        after.projectTransitioning ||
        after.projectGeneration !== before.projectGeneration ||
        after.activeTabId !== tabId
      )
        return;
      setWindowMode("editor");
    })();
  }, []);
  const revealSettings = useCallback(() => {
    setWindowMode("editor");
    void useWorkbenchStore.getState().openSettings();
  }, []);
  const drawerOpen =
    isCompactDrawerOpen(compact, sidebarOpen, windowMode) ||
    (windowMode === "agent" && agentLayout.artifactOverlay);
  const bodyStyle =
    windowMode === "agent"
      ? { gridTemplateColumns: agentLayout.gridTemplateColumns }
      : compact
        ? undefined
        : {
            gridTemplateColumns: `${editorToolRailWidth + EDITOR_ACTIVITY_BAR_WIDTH}px 8px minmax(0, 1fr)`,
          };
  return (
    <main
      className={`workbench workbench--${windowMode}${compact ? " workbench--compact" : ""}${agentNavigationCollapsed ? " workbench--navigation-collapsed" : ""}`}
      style={workbenchGeometryStyle}
    >
      <TitleBar
        mode={windowMode}
        onModeChange={setWindowMode}
        inert={resourceModal}
        agentNavigationCollapsed={agentNavigationCollapsed}
        onToggleAgentNavigation={() =>
          setAgentNavigationCollapsed((collapsed) => !collapsed)
        }
      />
      <AgentCenterKeeper windowMode={windowMode} />
      <EditorContinuityKeeper windowMode={windowMode} />
      <RendererRecreationPacketCollector windowMode={windowMode} />
      {windowMode === "agent" ? (
        <LazyModuleBoundary
          label="Agent workspace"
          load={loadAgentWorkbenchBody}
          render={(AgentWorkbenchBody) => (
            <AgentWorkbenchBody
              layout={agentLayout}
              style={bodyStyle}
              drawerOpen={drawerOpen}
              navigationCollapsed={agentNavigationCollapsed}
              artifactExpanded={agentArtifactExpanded}
              onArtifactExpandedChange={changeAgentArtifact}
              onAgentLeftRailWidthChange={(width) =>
                changeAgentSidebarWidth("left", width)
              }
              onOpenTerminal={() => setTerminalOpen(true)}
              onAgentArtifactWidthChange={(width) =>
                changeAgentSidebarWidth("artifact", width)
              }
              onOpenArtifactInEditor={openArtifactInEditor}
              onOpenSettings={revealSettings}
            />
          )}
        />
      ) : (
        <div className="workbench__body" style={bodyStyle}>
          <RightSidebar compact={compact} railFirst />
          {!compact ? (
            <WorkbenchSidebarSplitter
              ariaLabel="Resize Editor tool rail"
              side="leading"
              value={editorToolRailWidth}
              width={EDITOR_TOOL_RAIL_WIDTH}
              pointerOffsetAdjustment={EDITOR_ACTIVITY_BAR_WIDTH}
              onChange={setEditorToolRailWidth}
            />
          ) : null}
          <div
            className="center-region"
            inert={drawerOpen}
            aria-hidden={drawerOpen || undefined}
          >
            <div className="workbench__tabs">
              <TabStrip kinds={[...EDITOR_CANVAS_KINDS]} showNewChat={false} />
            </div>
            <ActiveEditorCanvas onOpenAgent={() => setWindowMode("agent")} />
          </div>
        </div>
      )}
      {terminalOpen ? (
        <LazyModuleBoundary
          label="Terminal"
          load={loadTerminalDock}
          render={(TerminalDock) => (
            <TerminalDock
              setOpen={setTerminalOpen}
              inert={resourceModal}
              pinnedChatId={pinnedAgentChatOwner()}
            />
          )}
        />
      ) : null}
      <StatusBar
        inert={resourceModal}
        railLeading={windowMode === "editor"}
        showSidebarToggle={windowMode === "editor"}
        showEditorSettings={windowMode === "editor"}
        onOpenTerminal={
          !terminalOpen && isTauriRuntime()
            ? () => setTerminalOpen(true)
            : undefined
        }
      />
      <DirtyTabDialog />
    </main>
  );
}
