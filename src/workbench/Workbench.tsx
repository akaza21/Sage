import {
  Activity,
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
import { useDocumentVisible } from "../ui/useDocumentVisible";
import { useRetainedElement } from "../ui/useRetainedElement";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import "./harness-browser-mode.css";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";
import { RightSidebar } from "./RightSidebar";
import { StatusBar } from "./StatusBar";
import type { TabStripProps } from "./TabStrip";
import { TitleBar } from "./TitleBar";
import { useAgentDesktopSidebarWidths } from "./useAgentDesktopSidebarWidths";
import { useCompactWorkbenchLayout } from "./useCompactWorkbenchLayout";
import { useHistoryKeys } from "./useHistoryKeys";
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
} from "./workbenchSidebarResize";
import { WorkbenchSidebarSplitter } from "./WorkbenchSidebarSplitter";
import {
  DEFAULT_WORKBENCH_WINDOW_MODE,
  EDITOR_CANVAS_KINDS,
  isCompactDrawerOpen,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

registerWorkbenchSurfaces();

const loadAgentWorkbenchBody = () => import("./AgentWorkbenchBody");

const loadRailStatus = () => import("./useAgentRailStatus");

const loadEditorCanvas = () => import("./ActiveEditorCanvas");

const loadQuickOpenHost = () => import("../features/quickOpen/QuickOpenHost");

const loadDirtyTabDialog = () => import("./DirtyTabDialog");

// The Editor's tab strip stays off the Agent startup path.
const loadTabStrip = () => import("./TabStrip");

const loadTerminalDock = () => import("./TerminalDockShell");

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
  const narrowAgent = useCompactWorkbenchLayout("(max-width: 600px)");
  const [initialLayout] = useState(readWorkbenchLayout);
  const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>(
    initialLayout.windowMode ?? DEFAULT_WORKBENCH_WINDOW_MODE,
  );
  const [agentVisited, setAgentVisited] = useState(windowMode === "agent");
  if (windowMode === "agent" && !agentVisited) setAgentVisited(true);
  const documentVisible = useDocumentVisible();
  const [agentArtifactExpanded, changeAgentArtifact] = useState(false);
  const [artifactMaximized, setArtifactMaximized] = useState(false);
  if (!agentArtifactExpanded && artifactMaximized) setArtifactMaximized(false);
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
    if (!collapsed && !agentLayout.navigationFits) changeAgentArtifact(false);
    setNavigation({ narrow: narrowAgent, collapsed });
  }, [
    agentLayout.navigationCollapsed,
    agentLayout.navigationFits,
    narrowAgent,
  ]);
  const agentVisible = windowMode === "agent" && !agentSettingsPage;
  const agentCenterVisible = !agentLayout.artifactOverlay && !artifactMaximized;
  const resourceModal = agentVisible && agentLayout.artifactModal;
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
  useHistoryKeys(agentSettingsPage ? null : windowMode);
  useRepositoryAutoRefresh();
  useEffect(() => {
    writeWorkbenchLayout({
      editorToolRailWidth,
      windowMode,
    });
  }, [editorToolRailWidth, windowMode]);
  const enterEditor = useCallback(() => setWindowMode("editor"), []);
  const [terminalContainer, terminalSlotRef] =
    useRetainedElement<HTMLDivElement>();
  const dockSlot = useMemo(
    () => <div className="center-region__dock" ref={terminalSlotRef} />,
    [terminalSlotRef],
  );
  const terminalDockSlot = terminalVisited ? dockSlot : null;
  // Keep the sleeping presentation's props stable across visibility changes.
  const agentBodyProps = useMemo(
    () => ({
      bottomPanel: terminalDockSlot,
      onPreviewTargetChange,
      layout: agentLayout,
      style: { gridTemplateColumns: agentLayout.gridTemplateColumns },
      drawerOpen: agentLayout.artifactOverlay,
      navigationCollapsed: agentLayout.navigationCollapsed,
      onExpandNavigation: toggleAgentNavigation,
      centerVisible: agentCenterVisible,
      artifactMaximized,
      onArtifactMaximizedChange: setArtifactMaximized,
      artifactExpanded: agentArtifactExpanded,
      onArtifactExpandedChange: changeAgentArtifact,
      onAgentLeftRailWidthChange: (width: number) =>
        changeAgentSidebarWidth("left", width),
      onOpenTerminal: terminal.open,
      onAgentArtifactWidthChange: (width: number) =>
        changeAgentSidebarWidth("artifact", width),
      onEnterEditor: enterEditor,
    }),
    [
      agentArtifactExpanded,
      artifactMaximized,
      agentCenterVisible,
      agentLayout,
      changeAgentArtifact,
      changeAgentSidebarWidth,
      enterEditor,
      onPreviewTargetChange,
      terminal.open,
      toggleAgentNavigation,
      terminalDockSlot,
    ],
  );
  const editorToolMode = compact ? "overlay" : "docked";
  const drawerOpen = isCompactDrawerOpen(
    editorToolMode === "overlay",
    sidebarOpen,
    windowMode,
  );
  const editorCenterVisible = !drawerOpen;
  // The retained terminal session follows the region hosting its portal, not
  // the persisted open preference. Agent overlays cover the entire center;
  // Editor adopts the same dock unless its tool drawer covers that center.
  const terminalVisible =
    terminal.visible &&
    documentVisible &&
    !agentSettingsPage &&
    (windowMode === "agent" ? agentCenterVisible : editorCenterVisible);
  const bodyStyle = compact
    ? undefined
    : {
        gridTemplateColumns: `${editorToolRailWidth + EDITOR_ACTIVITY_BAR_WIDTH}px 8px minmax(0, 1fr)`,
      };
  return (
    <main
      className={`workbench workbench--${windowMode}${compact ? " workbench--compact" : ""}${agentLayout.navigationCollapsed ? " workbench--navigation-collapsed" : ""}`}
      style={workbenchGeometryStyle}
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
            <Observer visible={agentVisible && !agentNavigationCollapsed} />
          )}
        />
      ) : null}
      <AgentCenterKeeper
        windowMode={agentSettingsPage ? "editor" : windowMode}
      />
      <EditorContinuityKeeper windowMode={windowMode} />
      <RendererRecreationPacketCollector windowMode={windowMode} />
      {agentVisited ? (
        <LazyModuleBoundary
          label="Agent workspace"
          load={loadAgentWorkbenchBody}
          render={(Host) => (
            <Host
              {...agentBodyProps}
              visible={agentVisible && documentVisible}
            />
          )}
        />
      ) : null}
      {agentSettingsPage ? (
        <LazyModuleBoundary
          label="Agent settings"
          load={loadAgentSettings}
          render={(AgentSettingsSurface) => (
            <AgentSettingsSurface chatRuntimeOwned={agentVisited} />
          )}
        />
      ) : null}
      {windowMode === "editor" && !agentSettingsPage ? (
        <Activity mode={documentVisible ? "visible" : "hidden"}>
          <div
            className="workbench__body"
            style={bodyStyle}
            data-editor-tools={editorToolMode}
          >
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
              <Activity mode={editorCenterVisible ? "visible" : "hidden"}>
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
                {terminalDockSlot}
              </Activity>
            </div>
            <LazyModuleBoundary
              label="Quick Open controller"
              load={loadQuickOpenHost}
              fallback={null}
              render={(QuickOpenHost) => <QuickOpenHost />}
            />
          </div>
        </Activity>
      ) : null}
      {terminalVisited ? (
        <LazyModuleBoundary
          label="Terminal"
          load={loadTerminalDock}
          render={(TerminalDock) => (
            <TerminalDock
              container={terminalContainer}
              setOpen={terminal.setOpen}
              visible={terminalVisible}
              open={terminal.visible}
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
