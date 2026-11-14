import type { ReactNode } from "react";
import type { projectTaskResources } from "./agentArtifactTabPresentation";
import { SubagentResourceRows } from "./SubagentResourceRows";

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
      <SubagentResourceRows agents={recentAgents} onSelect={onSelect} />
    </section>
  );
}
