import { useEffect, useState } from "react";

import { useWorkbenchStore } from "../app/store";
import { sameResource } from "../domain/resourceIdentity";
import { DirtyTabDialog } from "./DirtyTabDialog";
import { LeftSidebar } from "./LeftSidebar";
import { PlaceholderSurface } from "./PlaceholderSurface";
import { RightSidebar } from "./RightSidebar";
import { StatusBar } from "./StatusBar";
import { TabStrip } from "./TabStrip";
import { renderRegisteredCanvas } from "./surfaceRegistry";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";
import { TitleBar } from "./TitleBar";
import {
  agentCenterChatAction,
  DEFAULT_WORKBENCH_WINDOW_MODE,
  isCompactDrawerOpen,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";
import "./workbench-window-mode.css";
import { useCompactWorkbenchLayout } from "./useCompactWorkbenchLayout";

registerWorkbenchSurfaces();

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

function ActiveCanvas() {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const repositoryMutationError = useWorkbenchStore(
    (state) => state.repositoryMutationError,
  );
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  const contextualMutationError =
    activeTab?.resource &&
    (activeTab.kind === "file" || activeTab.kind === "diff") &&
    repositoryMutationError &&
    sameResource(activeTab.resource, repositoryMutationError.resource)
      ? repositoryMutationError
      : null;
  const canvas =
    activeTab?.kind === "chat" ||
    activeTab?.kind === "diff" ||
    activeTab?.kind === "file"
      ? renderRegisteredCanvas(activeTab.kind, activeTab)
      : null;

  return (
    <div className="active-canvas" role="tabpanel">
      {contextualMutationError ? (
        <div className="repository-mutation-alert" role="alert">
          <strong>{repositoryMutationTitle(contextualMutationError)}</strong>
          <span>{contextualMutationError.message}</span>
        </div>
      ) : null}
      {canvas}
      {activeTab &&
      activeTab.kind !== "chat" &&
      activeTab.kind !== "diff" &&
      activeTab.kind !== "file" ? (
        <PlaceholderSurface tab={activeTab} />
      ) : null}
      {!activeTab ? (
        <section className="placeholder-surface">
          <h1>No open tabs</h1>
          <p>
            Use New chat, or open a file from Explorer, Search, or Source
            Control.
          </p>
        </section>
      ) : null}
    </div>
  );
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

  useEffect(() => {
    if (windowMode !== "agent") return;
    if (projectTransitioning) return;
    const state = useWorkbenchStore.getState();
    const decision = agentCenterChatAction(state.tabs, activeTabId);
    if (decision.action === "activate") {
      void state.activateTab(decision.tabId);
    } else if (decision.action === "create") {
      void state.createChat();
    }
  }, [windowMode, activeTabId, projectTransitioning, projectGeneration]);

  return null;
}

export function Workbench() {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const compact = useCompactWorkbenchLayout();
  const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>(
    DEFAULT_WORKBENCH_WINDOW_MODE,
  );
  const drawerOpen = isCompactDrawerOpen(compact, sidebarOpen, windowMode);

  return (
    <main className={`workbench workbench--${windowMode}`}>
      <TitleBar mode={windowMode} onModeChange={setWindowMode} />
      <AgentCenterKeeper windowMode={windowMode} />
      <div className="workbench__body">
        <LeftSidebar />
        <div
          className="center-region"
          inert={drawerOpen}
          aria-hidden={drawerOpen || undefined}
        >
          <div className="workbench__tabs">
            <TabStrip />
          </div>
          <ActiveCanvas />
        </div>
        {windowMode === "editor" ? <RightSidebar compact={compact} /> : null}
      </div>
      <StatusBar />
      <DirtyTabDialog />
    </main>
  );
}
