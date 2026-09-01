import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";

import { useWorkbenchStore } from "../app/store";
import { useEditorAppearanceStore } from "../app/editorAppearanceStore";
import { sameResource } from "../domain/resourceIdentity";
import { admitsFocusedCodexRendererRecreation } from "../features/chat/acp/rendererRecreationCollectorAdmission";
import { mountRendererRecreationPacketCollector } from "../features/chat/acp/rendererRecreationPacket";
import { AgentArtifactPane } from "./AgentArtifactPane";
import { AgentChatPanes } from "./AgentChatPanes";
import { DirtyTabDialog } from "./DirtyTabDialog";
import { EditorWelcome } from "./EditorWelcome";
import { LeftSidebar } from "./LeftSidebar";
import { PlaceholderSurface } from "./PlaceholderSurface";
import { RightSidebar } from "./RightSidebar";
import { StatusBar } from "./StatusBar";
import { SettingsSurface } from "./SettingsSurface";
import { TabStrip } from "./TabStrip";
import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import { renderFileOrDiffCanvas } from "./surfaceRegistry";
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
  AGENT_ARTIFACT_WIDTH,
  AGENT_LEFT_RAIL_WIDTH,
  EDITOR_ACTIVITY_BAR_WIDTH,
  EDITOR_TOOL_RAIL_WIDTH,
} from "./workbenchSidebarResize";
import {
  readWorkbenchLayout,
  writeWorkbenchLayout,
} from "./workbenchLayoutStorage";
import "./workbench-window-mode.css";
import { useCompactWorkbenchLayout } from "./useCompactWorkbenchLayout";

registerWorkbenchSurfaces();

const workbenchGeometryStyle = {
  "--editor-activity-width": `${EDITOR_ACTIVITY_BAR_WIDTH}px`,
} as CSSProperties;

function repositoryMutationTitle(
  error: NonNullable<
    ReturnType<typeof useWorkbenchStore.getState>["repositoryMutationError"]
  >,
) {
  const path = error.resource.relativePath;
  if (error.kind === "containment") {
    return `Source Control containment blocked ${error.action} for ${path}`;
  }
  if (error.kind === "effectUnknown") {
    return `Source Control could not confirm ${error.action} for ${path}`;
  }
  return `Source Control could not ${error.action} ${path}`;
}

function ActiveCanvas({ onOpenAgent }: { onOpenAgent: () => void }) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const repositoryMutationError = useWorkbenchStore(
    (state) => state.repositoryMutationError,
  );
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  const editorTab =
    activeTab && isAgentArtifactTabKind(activeTab.kind) ? activeTab : null;
  const contextualMutationError =
    editorTab?.resource &&
    repositoryMutationError &&
    sameResource(editorTab.resource, repositoryMutationError.resource)
      ? repositoryMutationError
      : null;
  const canvas = editorTab ? renderFileOrDiffCanvas(editorTab) : null;
  const showEditorEmpty =
    !editorTab && (!activeTab || activeTab.kind === "chat");

  return (
    <div className="active-canvas" role="tabpanel">
      {contextualMutationError ? (
        <div className="repository-mutation-alert" role="alert">
          <strong>{repositoryMutationTitle(contextualMutationError)}</strong>
          <span>{contextualMutationError.message}</span>
        </div>
      ) : null}
      {canvas}
      {activeTab?.kind === "settings" ? <SettingsSurface /> : null}
      {activeTab &&
      !editorTab &&
      activeTab.kind !== "chat" &&
      activeTab.kind !== "settings" ? (
        <PlaceholderSurface tab={activeTab} />
      ) : null}
      {showEditorEmpty ? <EditorWelcome onOpenAgent={onOpenAgent} /> : null}
    </div>
  );
}

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
  const compact = useCompactWorkbenchLayout();
  const [initialLayout] = useState(readWorkbenchLayout);
  const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>(
    initialLayout.windowMode ?? DEFAULT_WORKBENCH_WINDOW_MODE,
  );
  const [agentLeftRailWidth, setAgentLeftRailWidth] = useState(
    AGENT_LEFT_RAIL_WIDTH.defaultWidth,
  );
  const [agentArtifactWidth, setAgentArtifactWidth] = useState(
    AGENT_ARTIFACT_WIDTH.defaultWidth,
  );
  const [agentArtifactExpanded, setAgentArtifactExpanded] = useState(false);
  const [agentNavigationOpen, setAgentNavigationOpen] = useState(false);
  const [editorToolRailWidth, setEditorToolRailWidth] = useState(
    initialLayout.editorToolRailWidth,
  );
  useEffect(() => {
    void useEditorAppearanceStore.getState().restore();
  }, []);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === ",") {
        event.preventDefault();
        setWindowMode("editor");
        useWorkbenchStore.getState().openSettings();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
  useEffect(() => {
    writeWorkbenchLayout({
      editorToolRailWidth,
      windowMode,
    });
  }, [editorToolRailWidth, windowMode]);
  const changeAgentNavigation = useCallback((open: boolean) => {
    setAgentNavigationOpen(open);
    if (open) setAgentArtifactExpanded(false);
  }, []);
  const changeAgentArtifact = useCallback((expanded: boolean) => {
    setAgentArtifactExpanded(expanded);
    if (expanded) setAgentNavigationOpen(false);
  }, []);
  const drawerOpen =
    isCompactDrawerOpen(compact, sidebarOpen, windowMode) ||
    (compact &&
      windowMode === "agent" &&
      (agentNavigationOpen || agentArtifactExpanded));
  const bodyStyle = compact
    ? undefined
    : windowMode === "agent"
      ? agentArtifactExpanded
        ? {
            gridTemplateColumns: `${agentLeftRailWidth}px 8px minmax(0, 1fr) 8px ${agentArtifactWidth}px`,
          }
        : {
            gridTemplateColumns: `${agentLeftRailWidth}px 8px minmax(0, 1fr) 44px`,
          }
      : {
          gridTemplateColumns: `${editorToolRailWidth + EDITOR_ACTIVITY_BAR_WIDTH}px 8px minmax(0, 1fr)`,
        };

  return (
    <main
      className={`workbench workbench--${windowMode}${compact ? " workbench--compact" : ""}`}
      style={workbenchGeometryStyle}
    >
      <TitleBar mode={windowMode} onModeChange={setWindowMode} />
      <AgentCenterKeeper windowMode={windowMode} />
      <EditorContinuityKeeper windowMode={windowMode} />
      <RendererRecreationPacketCollector windowMode={windowMode} />
      <div className="workbench__body" style={bodyStyle}>
        {windowMode === "editor" ? (
          <RightSidebar compact={compact} railFirst />
        ) : (
          <LeftSidebar
            compact={compact}
            open={agentNavigationOpen}
            onOpenChange={changeAgentNavigation}
          />
        )}
        {!compact ? (
          <WorkbenchSidebarSplitter
            ariaLabel={
              windowMode === "agent"
                ? "Resize project and chat rail"
                : "Resize Editor tool rail"
            }
            side="leading"
            value={
              windowMode === "agent" ? agentLeftRailWidth : editorToolRailWidth
            }
            width={
              windowMode === "agent"
                ? AGENT_LEFT_RAIL_WIDTH
                : EDITOR_TOOL_RAIL_WIDTH
            }
            pointerOffsetAdjustment={
              windowMode === "editor" ? EDITOR_ACTIVITY_BAR_WIDTH : 0
            }
            onChange={
              windowMode === "agent"
                ? setAgentLeftRailWidth
                : setEditorToolRailWidth
            }
          />
        ) : null}
        <div
          className="center-region"
          inert={drawerOpen}
          aria-hidden={drawerOpen || undefined}
        >
          {windowMode === "agent" ? (
            <AgentChatPanes compact={compact} />
          ) : (
            <>
              <div className="workbench__tabs">
                <TabStrip
                  kinds={[...EDITOR_CANVAS_KINDS]}
                  showNewChat={false}
                />
              </div>
              <ActiveCanvas onOpenAgent={() => setWindowMode("agent")} />
            </>
          )}
        </div>
        {windowMode === "agent" ? (
          <>
            {!compact && agentArtifactExpanded ? (
              <WorkbenchSidebarSplitter
                ariaLabel="Resize files and preview"
                side="trailing"
                value={agentArtifactWidth}
                width={AGENT_ARTIFACT_WIDTH}
                onChange={setAgentArtifactWidth}
              />
            ) : null}
            <AgentArtifactPane
              expanded={agentArtifactExpanded}
              focusOnExpand={compact}
              onExpandedChange={changeAgentArtifact}
            />
          </>
        ) : null}
      </div>
      <StatusBar
        railLeading={windowMode === "editor"}
        showSidebarToggle={windowMode === "editor"}
        showEditorSettings={windowMode === "editor"}
      />
      <DirtyTabDialog />
    </main>
  );
}
