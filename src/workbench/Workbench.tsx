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

export function Workbench() {
  const project = useWorkbenchStore((state) => state.project);
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const compact = useCompactWorkbenchLayout();
  const drawerOpen = compact && sidebarOpen;

  return (
    <main className="workbench">
      <header className="title-bar">
        <div className="title-bar__brand">
          <span className="title-bar__logo">S</span>
          <strong>Sage</strong>
        </div>
        <div className="title-bar__project">
          {project?.name ?? "No project"}
        </div>
        <div className="title-bar__spacer" />
      </header>
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
        <RightSidebar compact={compact} />
      </div>
      <StatusBar />
      <DirtyTabDialog />
    </main>
  );
}
