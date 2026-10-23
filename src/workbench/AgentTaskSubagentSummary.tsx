import type { ReactNode } from "react";
import type { projectTaskResources } from "./agentArtifactTabPresentation";
import { SubagentResourceRows } from "./SubagentResourceRows";

/** Lazy Subagents region around the eagerly rendered counts. */
export function AgentTaskSubagentSummary({
  recentAgents,
  working,
  counts,
  onSelect,
}: Pick<ReturnType<typeof projectTaskResources>, "recentAgents"> & {
  working: number;
  counts: (glyphs: ReactNode) => ReactNode;
  onSelect: () => void;
}) {
  return (
    <section
      aria-label="Subagents"
      title="Up to 128 subagents from the latest 128 turns and 256 items total"
    >
      <strong
        role="heading"
        aria-level={3}
        aria-label={`Subagents: ${recentAgents.length}`}
      >
        Subagents {recentAgents.length}
      </strong>
      {counts(
        <span className="agent-task-header__agent-glyphs" aria-hidden="true">
          {Array.from({ length: Math.min(working, 3) }, (_, index) => (
            <span
              className={`agent-task-header__agent-glyph agent-task-header__agent-glyph--${index}`}
              key={index}
            >
              ✦
            </span>
          ))}
        </span>,
      )}
      <SubagentResourceRows agents={recentAgents} onSelect={onSelect} />
      <small>Latest retained observations; not live.</small>
    </section>
  );
}
