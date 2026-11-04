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
import { QuickOpenHost } from "../features/quickOpen/QuickOpenHost";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { isTauriRuntime } from "../ports/nativeInvoke";
import { useRetainedElement } from "../ui/useRetainedElement";
import { LazyModuleBoundary, RetainedActivity } from "../ui/LazyModuleBoundary";
import { DirtyTabDialog } from "./DirtyTabDialog";
import "./harness-browser-mode.css";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";
import { RightSidebar } from "./RightSidebar";
import { StatusBar } from "./StatusBar";
import type { TabStripProps } from "./TabStrip";
import { TitleBar } from "./TitleBar";
import { useAgentDesktopSidebarWidths } from "./useAgentDesktopSidebarWidths";
import { useCompactWorkbenchLayout } from "./useCompactWorkbenchLayout";
import { useEditorHistoryKeys } from "./useEditorHistoryKeys";
import { useRepositoryAutoRefresh } from "./useRepositoryAutoRefresh";
import { useWorkbenchKeys } from "./useWorkbenchKeys";
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
  isAgentArtifactTabKind,
  isCompactDrawerOpen,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

registerWorkbenchSurfaces();

const loadAgentWorkbenchBody = () => import("./AgentWorkbenchBody");

const loadRailStatus = () =>
  import("./useAgentRailStatus").then(({ AgentRailStatusObserver }) => ({
    default: AgentRailStatusObserver,
  }));

const loadEditorCanvas = () => import("./ActiveEditorCanvas");

// The Editor's tab strip stays off the Agent startup path.
const loadTabStrip = () =>
  import("./TabStrip").then(({ TabStrip }) => ({ default: TabStrip }));

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
  const [terminalOpen, setTerminalOpen] = useState(false);
  const compact = useCompactWorkbenchLayout();
  const narrowAgent = useCompactWorkbenchLayout("(max-width: 600px)");
  const [initialLayout] = useState(readWorkbenchLayout);
  const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>(
    initialLayout.windowMode ?? DEFAULT_WORKBENCH_WINDOW_MODE,
  );
  const [agentArtifactExpanded, changeAgentArtifact] = useState(false);
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
  const resourceModal = agentVisible && agentLayout.artifactModal;
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
  const openArtifactInEditor = useCallback(async (tabId: string) => {
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
  }, []);
  const [terminalContainer, terminalSlotRef] =
    useRetainedElement<HTMLDivElement>();
  const dockSlot = useMemo(
    () => <div className="center-region__dock" ref={terminalSlotRef} />,
    [terminalSlotRef],
  );
  const terminalDockSlot = terminalOpen ? dockSlot : null;
  // Memoized so a sleeping Agent body is not re-rendered by Workbench updates.
  const agentBody = useMemo(
    () => (
      <LazyModuleBoundary
        label="Agent workspace"
        load={loadAgentWorkbenchBody}
        render={(AgentWorkbenchBody) => (
          <AgentWorkbenchBody
            bottomPanel={terminalDockSlot}
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
            onOpenTerminal={() => setTerminalOpen(true)}
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
      toggleAgentNavigation,
      terminalDockSlot,
    ],
  );
  const drawerOpen = isCompactDrawerOpen(compact, sidebarOpen, windowMode);
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
      <RetainedActivity visible={agentVisible}>{agentBody}</RetainedActivity>
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
            {terminalDockSlot}
          </div>
          <QuickOpenHost />
        </div>
      ) : null}
      {terminalOpen ? (
        <LazyModuleBoundary
          label="Terminal"
          load={loadTerminalDock}
          render={(TerminalDock) => (
            <TerminalDock
              container={terminalContainer}
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
