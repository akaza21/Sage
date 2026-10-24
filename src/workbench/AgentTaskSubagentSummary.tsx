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
      title="The latest 128 subagents found in the latest 256 items of the latest 128 turns"
    >
      {/* A plain heading; the count row below says how many are working
          (Codex s4/f0111). */}
      <strong role="heading" aria-level={3}>
        Subagents
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
      {/* Rows come from the parent page's subagent states and re-render with
          every page load, so they move as the parent projection changes. */}
      <SubagentResourceRows agents={recentAgents} onSelect={onSelect} />
    </section>
  );
}
