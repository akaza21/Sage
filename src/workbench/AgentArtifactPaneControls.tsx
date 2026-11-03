import { Files, Maximize2, PanelRightClose, Plus } from "../ui/leanIcons";
export function AgentArtifactEmptyState({ titleId }: { titleId: string }) {
  return (
    <section
      className="placeholder-surface agent-artifact-pane__empty-state"
      aria-labelledby={titleId}
    >
      <span className="placeholder-surface__icon" aria-hidden="true">
        <Files size={19} strokeWidth={1.7} />
      </span>
      <h1 id={titleId}>Waiting for this Chat</h1>
      <p>
        Open a file or change from the conversation. Preview will appear when
        this Chat starts a loopback server.
      </p>
    </section>
  );
}

export function AgentArtifactPaneControls({
  maximized,
  onMaximizedChange,
  disabled,
  onNewTab,
  onCollapse,
}: {
  maximized: boolean;
  onMaximizedChange: (maximized: boolean) => void;
  disabled: boolean;
  onNewTab: () => void;
  onCollapse: () => void;
}) {
  return (
    <>
      <button
        className="agent-artifact-pane__collapse agent-artifact-pane__new-tab"
        type="button"
        aria-label="New browser tab"
        title="New browser tab"
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
