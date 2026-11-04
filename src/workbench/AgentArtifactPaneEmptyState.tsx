import { Files } from "../ui/leanIcons";

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
