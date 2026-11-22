import { FileCode2 } from "../ui/leanIcons";
import { useWorkbenchStore } from "../app/store";
import { type WorkbenchTab } from "../domain/workbench";
import type { RepositoryMutationError } from "../app/repositoryMutationSlice";
import { isRootRepository } from "../domain/repositoryInventory";
import { resourceKey, sameResource } from "../domain/resourceIdentity";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import { Notice } from "../ui/Notice";
import { EditorWelcome } from "./EditorWelcome";
import { renderFileOrDiffCanvas } from "./surfaceRegistry";
import { isAgentArtifactTabKind } from "./workbenchWindowMode";
import "./active-editor-canvas.css";

const loadSettingsSurface = () =>
  import("./SettingsSurface").then(({ SettingsSurface }) => ({
    default: SettingsSurface,
  }));

const loadSaveAllNotice = () =>
  import("../features/files/SaveAllNotice").then(({ SaveAllNotice }) => ({
    default: SaveAllNotice,
  }));

export default ActiveEditorCanvas;

/** Source Control owns the explanation; the editor only points there. */
function RepositoryMutationLink({ error }: { error: RepositoryMutationError }) {
  return (
    <Notice
      identity={`repository-mutation-link:${resourceKey(error.resource)}:${error.action}`}
      severity="warning"
      message="Source Control has a problem with this file."
      action={{
        label: "Show in Source Control",
        onClick: () => {
          const state = useWorkbenchStore.getState();
          state.setSidebarView("sourceControl");
          const selected = state.selectedRepository;
          if (
            selected &&
            !(
              selected.rootId === error.resource.rootId &&
              isRootRepository(selected)
            )
          ) {
            void state.selectRepository({
              rootId: error.resource.rootId,
              relativePath: "",
            });
          }
        },
      }}
    />
  );
}

function PlaceholderSurface({ tab }: { tab: WorkbenchTab }) {
  return (
    <section className="placeholder-surface">
      <FileCode2 size={32} strokeWidth={1.4} aria-hidden="true" />
      <h1>{tab.title}</h1>
      <p>
        This legacy tab type is unavailable in this beta. Close it and use the
        current workbench controls.
      </p>
    </section>
  );
}

export function ActiveEditorCanvas({
  onOpenAgent,
}: {
  onOpenAgent: () => void;
}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const repositoryMutationError = useWorkbenchStore(
    (state) => state.repositoryMutationError,
  );
  const sourceControlOpen = useWorkbenchStore(
    (state) => state.sidebarOpen && state.sidebarView === "sourceControl",
  );
  const saveAllFailed = useWorkbenchStore(
    (state) =>
      !!state.saveAllReport &&
      state.saveAllReport.projectId === state.project?.id,
  );
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  const editorTab =
    activeTab && isAgentArtifactTabKind(activeTab.kind) ? activeTab : null;
  const contextualMutationError =
    editorTab?.resource &&
    repositoryMutationError &&
    !sourceControlOpen &&
    sameResource(editorTab.resource, repositoryMutationError.resource)
      ? repositoryMutationError
      : null;
  const canvas = editorTab ? renderFileOrDiffCanvas(editorTab) : null;
  const showEditorEmpty =
    !editorTab &&
    (!activeTab || activeTab.kind === "chat" || activeTab.kind === "browser");

  return (
    <div
      className={`active-canvas${contextualMutationError || saveAllFailed ? " active-canvas--noticed" : ""}`}
      role="tabpanel"
    >
      {contextualMutationError ? (
        <RepositoryMutationLink error={contextualMutationError} />
      ) : null}
      {saveAllFailed ? (
        <LazyModuleBoundary
          label="Save All"
          load={loadSaveAllNotice}
          fallback={null}
          render={(SaveAllNotice) => <SaveAllNotice />}
        />
      ) : null}
      {canvas}
      {activeTab?.kind === "settings" ? (
        <LazyModuleBoundary
          label="Settings"
          load={loadSettingsSurface}
          render={(SettingsSurface) => <SettingsSurface />}
        />
      ) : null}
      {activeTab &&
      !editorTab &&
      activeTab.kind !== "chat" &&
      activeTab.kind !== "settings" &&
      activeTab.kind !== "browser" ? (
        <PlaceholderSurface tab={activeTab} />
      ) : null}
      {showEditorEmpty ? <EditorWelcome onOpenAgent={onOpenAgent} /> : null}
    </div>
  );
}
