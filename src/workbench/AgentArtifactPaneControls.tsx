import { Maximize2, PanelRightClose, Plus } from "../ui/leanIcons";
import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { create } from "zustand";

const useBrowserPaneError = create(() => ({
  error: null as { projectId: string; message: string } | null,
}));

// Mirrors the native registry; native admission remains authoritative.
const MAX_BROWSER_TABS = 8;
const BROWSER_TAB_LIMIT_MESSAGE =
  "Browser tab limit reached. Close a browser tab to open another.";
function browserTabCapacity(tabs: readonly WorkbenchTab[]) {
  return (
    tabs.filter((tab) => tab.kind === "browser" && tab.status !== "closed")
      .length < MAX_BROWSER_TABS
  );
}

function useBrowserMessage() {
  const error = useBrowserPaneError((state) => state.error);
  return useWorkbenchStore((state) => {
    if (!state.project || state.projectTransitioning) return null;
    if (!browserTabCapacity(state.tabs)) return BROWSER_TAB_LIMIT_MESSAGE;
    return error?.projectId === state.project.id &&
      error.message !== BROWSER_TAB_LIMIT_MESSAGE
      ? error.message
      : null;
  });
}

export function AgentBrowserPaneStatus() {
  const message = useBrowserMessage();
  return message ? <p role="status">{message}</p> : null;
}

/** Opens a browser tab in the pane; a refusal shows in the pane status. */
export function openPaneBrowser(url?: string) {
  const projectId = useWorkbenchStore.getState().project?.id;
  useBrowserPaneError.setState({ error: null });
  void useWorkbenchStore
    .getState()
    .openBrowser(url)
    .then((message) => {
      if (message && projectId)
        useBrowserPaneError.setState({ error: { projectId, message } });
    });
}

export function AgentArtifactPaneControls({
  maximized,
  onMaximizedChange,
  disabled,
  launcherOpen = false,
  onNewTab,
  onCollapse,
}: {
  maximized: boolean;
  onMaximizedChange: (maximized: boolean) => void;
  disabled: boolean;
  launcherOpen?: boolean;
  onNewTab: () => void;
  onCollapse: () => void;
}) {
  return (
    <>
      <button
        className={`agent-artifact-pane__collapse agent-artifact-pane__new-tab${
          launcherOpen ? " agent-artifact-pane__new-tab--open" : ""
        }`}
        type="button"
        aria-label="New tab"
        aria-pressed={launcherOpen}
        title="New tab"
        disabled={disabled}
        onClick={onNewTab}
      >
        <Plus size={16} aria-hidden="true" />
      </button>
      <button
        className="agent-artifact-pane__collapse"
        type="button"
        aria-label={
          maximized ? "Restore resource pane" : "Expand resource pane"
        }
        title={maximized ? "Restore resource pane" : "Expand resource pane"}
        aria-pressed={maximized}
        onClick={() => onMaximizedChange(!maximized)}
      >
        <Maximize2 size={14} aria-hidden="true" />
      </button>
      <button
        className="agent-artifact-pane__collapse"
        type="button"
        aria-label="Collapse files, changes, and preview"
        title="Collapse resources"
        onClick={onCollapse}
      >
        <PanelRightClose size={16} aria-hidden="true" />
      </button>
    </>
  );
}
