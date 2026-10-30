import { FileCode2, Files, MessageSquare, Search } from "../ui/leanIcons";
import { useWorkbenchStore, type SidebarView } from "../app/store";
import { type WorkbenchTab } from "../domain/workbench";
import { sameResource } from "../domain/resourceIdentity";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import { renderFileOrDiffCanvas } from "./surfaceRegistry";
import { isAgentArtifactTabKind } from "./workbenchWindowMode";

const loadSettingsSurface = () =>
  import("./SettingsSurface").then(({ SettingsSurface }) => ({
    default: SettingsSurface,
  }));

export default ActiveEditorCanvas;

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
    !editorTab &&
    (!activeTab || activeTab.kind === "chat" || activeTab.kind === "browser");

  return (
    <div className="active-canvas" role="tabpanel">
      {contextualMutationError ? (
        <div className="repository-mutation-alert" role="alert">
          <strong>{repositoryMutationTitle(contextualMutationError)}</strong>
          <span>{contextualMutationError.message}</span>
        </div>
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

function EditorWelcome({ onOpenAgent }: { onOpenAgent: () => void }) {
  const setSidebarView = useWorkbenchStore((state) => state.setSidebarView);

  const openView = (view: SidebarView) => setSidebarView(view);

  return (
    <section className="editor-welcome" aria-label="Editor welcome">
      <div className="editor-welcome__mark" aria-hidden="true">
        S
      </div>
      <div className="editor-welcome__actions">
        <button type="button" onClick={() => openView("explorer")}>
          <Files size={16} aria-hidden="true" />
          Browse files
        </button>
        <button type="button" onClick={() => openView("search")}>
          <Search size={16} aria-hidden="true" />
          Search files
        </button>
        <button type="button" onClick={onOpenAgent}>
          <MessageSquare size={16} aria-hidden="true" />
          Open Agent
        </button>
      </div>
      <p>Use Settings to tune how every File is displayed.</p>
    </section>
  );
}
