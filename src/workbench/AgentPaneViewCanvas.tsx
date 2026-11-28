import { MermaidFigureBoundary } from "../features/chat/acp/MermaidFigureBoundary";
import type { AgentPaneView } from "./agentPaneViews";

/**
 * A diagram opens at full pane width. Subagent transcripts and background
 * task output are typed seams: they render a placeholder until the harness
 * reports a readable transcript or output stream.
 */
export function AgentPaneViewCanvas({ view }: { view: AgentPaneView }) {
  return (
    <section
      className={`agent-pane-view agent-pane-view--${view.kind}`}
      aria-label={view.title}
    >
      {view.kind === "diagram" && view.source !== undefined ? (
        <MermaidFigureBoundary source={view.source} />
      ) : (
        <p role="status">Not available for this harness yet.</p>
      )}
    </section>
  );
}
