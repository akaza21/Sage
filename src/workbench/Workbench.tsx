import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
} from "react";
import { pinnedAgentChatOwner } from "../app/agentChatOwner";
import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { useSessionAccessModeStore } from "../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../app/store";
import { useUiAppearanceStore } from "../app/uiAppearanceStore";
import { ExplorerReleasesOpenFiles } from "../features/files/explorerOpenFiles";
import { usePreviewTargetLifetime } from "../features/preview/usePreviewTargetLifetime";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { LazyModuleBoundary, RetainedActivity } from "../ui/LazyModuleBoundary";
import "./harness-browser-mode.css";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";
import { RightSidebar } from "./RightSidebar";
import { StatusBar } from "./StatusBar";
import type { TabStripProps } from "./TabStrip";
import { TitleBar } from "./TitleBar";
import { useAgentDesktopSidebarWidths } from "./useAgentDesktopSidebarWidths";
import { AGENT_INLINE_RESOURCE_MIN_WIDTH } from "./agentWorkbenchLayout";
import { useCompactWorkbenchLayout } from "./useCompactWorkbenchLayout";
import { useEditorHistoryKeys } from "./useEditorHistoryKeys";
import { useRepositoryAutoRefresh } from "./useRepositoryAutoRefresh";
import { useWorkbenchKeys, useTerminalDock } from "./useWorkbenchKeys";
import "./workbench-window-mode.css";
import {
  AgentCenterKeeper,
  EditorContinuityKeeper,
  RendererRecreationPacketCollector,
} from "./WorkbenchContinuity";
import {
  readWorkbenchLayout,
  writeWorkbenchLayout,
} from "./workbenchLayoutStorage";
import {
  EDITOR_ACTIVITY_BAR_WIDTH,
  EDITOR_TOOL_RAIL_WIDTH,
  WORKBENCH_SPLITTER_WIDTH,
} from "./workbenchSidebarResize";
import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import {
  DEFAULT_WORKBENCH_WINDOW_MODE,
  EDITOR_CANVAS_KINDS,
  isAgentArtifactTabKind,
  isCompactDrawerOpen,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

registerWorkbenchSurfaces();

const loadAgentWorkbenchBody = () =>
  import("./AgentWorkbenchBody").then(({ AgentWorkbenchBody }) => ({
    default: AgentWorkbenchBody,
  }));

const loadRailStatus = () =>
  import("./useAgentRailStatus").then(({ AgentRailStatusObserver }) => ({
    default: AgentRailStatusObserver,
  }));

const loadEditorCanvas = () => import("./ActiveEditorCanvas");

const loadQuickOpenHost = () => import("../features/quickOpen/QuickOpenHost");

const loadDirtyTabDialog = () => import("./DirtyTabDialog");

// The Editor's tab strip stays off the Agent startup path.
const loadTabStrip = () =>
  import("./TabStrip").then(({ TabStrip }) => ({ default: TabStrip }));

const loadTerminalDock = () =>
  import("./TerminalDockShell").then(({ TerminalDockForWorkbench }) => ({
    default: TerminalDockForWorkbench,
  }));

const loadAgentSettings = () =>
  import("../features/settings/AgentSettingsSurface");

const workbenchGeometryStyle = {
  "--editor-activity-width": `${EDITOR_ACTIVITY_BAR_WIDTH}px`,
} as CSSProperties;

export function Workbench() {
  const onPreviewTargetChange = usePreviewTargetLifetime();
  const agentSettingsPage = useAgentSettingsState(
    (state) => state.page !== null,
  );
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const pendingClose = useWorkbenchStore((state) => state.pendingCloseTabId);
  const compact = useCompactWorkbenchLayout();
  const narrowAgent = useCompactWorkbenchLayout(
    `(max-width: ${AGENT_INLINE_RESOURCE_MIN_WIDTH - 1}px)`,
  );
  const [initialLayout] = useState(readWorkbenchLayout);
  const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>(
    initialLayout.windowMode ?? DEFAULT_WORKBENCH_WINDOW_MODE,
  );
  const [agentArtifactExpanded, setAgentArtifactExpanded] = useState(false);
  const [navigation, setNavigation] = useState({
    narrow: narrowAgent,
    collapsed: narrowAgent,
  });
  const agentNavigationCollapsed =
    navigation.narrow === narrowAgent ? navigation.collapsed : narrowAgent;
  if (navigation.narrow !== narrowAgent)
    setNavigation({ narrow: narrowAgent, collapsed: narrowAgent });
  const { changeWidth: changeAgentSidebarWidth, layout: agentLayout } =
    useAgentDesktopSidebarWidths(
      agentArtifactExpanded,
      agentNavigationCollapsed,
    );
  const toggleAgentNavigation = useCallback(() => {
    const collapsed = !agentLayout.navigationCollapsed;
    if (!collapsed && !agentLayout.navigationFits)
      setAgentArtifactExpanded(false);
    setNavigation({ narrow: narrowAgent, collapsed });
  }, [
    agentLayout.navigationCollapsed,
    agentLayout.navigationFits,
    narrowAgent,
  ]);
  const resourceModal =
    windowMode === "agent" && !agentSettingsPage && agentLayout.artifactModal;
  const terminal = useTerminalDock(resourceModal);
  const [terminalVisited, setTerminalVisited] = useState(false);
  if (terminal.visible && !terminalVisited) setTerminalVisited(true);
  const [editorToolRailWidth, setEditorToolRailWidth] = useState(
    initialLayout.editorToolRailWidth,
  );
  useEffect(() => {
    void useEditorSettingsStore.getState().restore();
    void useSessionAccessModeStore.getState().restore();
    void useUiAppearanceStore.getState().restore();
  }, []);
  useWorkbenchKeys({
    resourceModal,
    windowMode,
    toggleAgentNavigation,
    setWindowMode,
  });
  useEditorHistoryKeys(windowMode === "editor");
  useRepositoryAutoRefresh();
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
  // Memoized so a sleeping Agent body is not re-rendered by Workbench updates.
  const agentBody = useMemo(
    () => (
      <LazyModuleBoundary
        label="Agent workspace"
        load={loadAgentWorkbenchBody}
        render={(AgentWorkbenchBody) => (
          <AgentWorkbenchBody
            onPreviewTargetChange={onPreviewTargetChange}
            layout={agentLayout}
            style={{ gridTemplateColumns: agentLayout.gridTemplateColumns }}
            drawerOpen={agentLayout.artifactOverlay}
            navigationCollapsed={agentLayout.navigationCollapsed}
            onExpandNavigation={toggleAgentNavigation}
            artifactExpanded={agentArtifactExpanded}
            onArtifactExpandedChange={changeAgentArtifact}
            onAgentLeftRailWidthChange={(width) =>
              changeAgentSidebarWidth("left", width)
            }
            onOpenTerminal={terminal.open}
            onAgentArtifactWidthChange={(width) =>
              changeAgentSidebarWidth("artifact", width)
            }
            onOpenArtifactInEditor={openArtifactInEditor}
          />
        )}
      />
    ),
    [
      agentArtifactExpanded,
      agentLayout,
      changeAgentArtifact,
      changeAgentSidebarWidth,
      openArtifactInEditor,
      onPreviewTargetChange,
      terminal.open,
      toggleAgentNavigation,
    ],
  );
  const drawerOpen = isCompactDrawerOpen(compact, sidebarOpen, windowMode);
  const editorRailWidth = editorToolRailWidth + EDITOR_ACTIVITY_BAR_WIDTH;
  const editorContentStart = compact
    ? EDITOR_ACTIVITY_BAR_WIDTH
    : editorRailWidth + WORKBENCH_SPLITTER_WIDTH;
  const bodyStyle = compact
    ? undefined
    : {
        gridTemplateColumns: `${editorRailWidth}px ${WORKBENCH_SPLITTER_WIDTH}px minmax(0, 1fr)`,
      };
  return (
    <main
      className={`workbench workbench--${windowMode}${compact ? " workbench--compact" : ""}${agentLayout.navigationCollapsed ? " workbench--navigation-collapsed" : ""}`}
      style={
        {
          ...workbenchGeometryStyle,
          "--workbench-content-start": `${windowMode === "agent" ? agentLayout.contentStart : editorContentStart}px`,
        } as CSSProperties
      }
    >
      <TitleBar
        mode={windowMode}
        onModeChange={setWindowMode}
        inert={resourceModal}
        agentNavigationCollapsed={agentLayout.navigationCollapsed}
        onToggleAgentNavigation={toggleAgentNavigation}
      />
      {isTauriRuntime() ? (
        <LazyModuleBoundary
          label="Chat status"
          load={loadRailStatus}
          fallback={null}
          render={(Observer) => (
            <Observer
              visible={
                windowMode === "agent" &&
                !agentSettingsPage &&
                !agentNavigationCollapsed
              }
            />
          )}
        />
      ) : null}
      <AgentCenterKeeper
        windowMode={agentSettingsPage ? "editor" : windowMode}
      />
      <EditorContinuityKeeper windowMode={windowMode} />
      <RendererRecreationPacketCollector windowMode={windowMode} />
      <RetainedActivity visible={windowMode === "agent" && !agentSettingsPage}>
        {agentBody}
      </RetainedActivity>
      {agentSettingsPage ? (
        <LazyModuleBoundary
          label="Agent settings"
          load={loadAgentSettings}
          render={(AgentSettingsSurface) => <AgentSettingsSurface />}
        />
      ) : null}
      {windowMode === "editor" && !agentSettingsPage ? (
        <div className="workbench__body" style={bodyStyle}>
          <ExplorerReleasesOpenFiles.Provider value={true}>
            <RightSidebar compact={compact} railFirst />
          </ExplorerReleasesOpenFiles.Provider>
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
            className="center-region surface surface--card"
            inert={drawerOpen}
            aria-hidden={drawerOpen || undefined}
          >
            <div className="workbench__tabs">
              <LazyModuleBoundary<TabStripProps>
                label="tabs"
                load={loadTabStrip}
                fallback={null}
                render={(TabStrip) => (
                  <TabStrip
                    kinds={[...EDITOR_CANVAS_KINDS]}
                    showNewChat={false}
                  />
                )}
              />
            </div>
            <LazyModuleBoundary
              label="Editor"
              load={loadEditorCanvas}
              render={(ActiveEditorCanvas) => (
                <ActiveEditorCanvas
                  onOpenAgent={() => setWindowMode("agent")}
                />
              )}
            />
          </div>
          <LazyModuleBoundary
            label="Quick Open controller"
            load={loadQuickOpenHost}
            fallback={null}
            render={(QuickOpenHost) => <QuickOpenHost />}
          />
        </div>
      ) : null}
      {terminalVisited ? (
        <LazyModuleBoundary
          label="Terminal"
          load={loadTerminalDock}
          render={(TerminalDock) => (
            <TerminalDock
              setOpen={terminal.setOpen}
              visible={terminal.visible}
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
        terminalControls={terminal.controls}
      />
      {pendingClose ? (
        <LazyModuleBoundary
          label="Unsaved file confirmation"
          load={loadDirtyTabDialog}
          render={(DirtyTabDialog) => <DirtyTabDialog />}
        />
      ) : null}
    </main>
  );
}

export default Workbench;
