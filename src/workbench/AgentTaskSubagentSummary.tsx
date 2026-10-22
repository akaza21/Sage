import { lazy, Suspense } from "react";
import type { projectTaskResources } from "./agentArtifactTabPresentation";

const SubagentResourceRows = lazy(() =>
  import("./SubagentSurface").then((module) => ({
    default: module.SubagentResourceRows,
  })),
);

export function AgentTaskSubagentSummary({
  recentAgents,
  workingAgents,
  doneAgents,
  onSelect,
}: Pick<
  ReturnType<typeof projectTaskResources>,
  "recentAgents" | "workingAgents" | "doneAgents"
> & { onSelect: () => void }) {
  return recentAgents.length > 0 ? (
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
      <div className="agent-task-header__agent-summary">
        <span
          className="agent-task-header__agent-metric"
          aria-label={`Recent subagents working: ${workingAgents}`}
        >
          <span className="agent-task-header__agent-glyphs" aria-hidden="true">
            {Array.from({ length: Math.min(workingAgents, 3) }, (_, index) => (
              <span
                className={`agent-task-header__agent-glyph agent-task-header__agent-glyph--${index}`}
                key={index}
              >
                ✦
              </span>
            ))}
          </span>
          {workingAgents} working
        </span>
        <span
          className="agent-task-header__agent-metric"
          aria-label={`Recent subagents done: ${doneAgents}`}
        >
          {doneAgents} done
        </span>
      </div>
      <Suspense fallback={<p role="status">Loading subagents…</p>}>
        <SubagentResourceRows agents={recentAgents} onSelect={onSelect} />
      </Suspense>
      <small>Latest retained observations; not live.</small>
    </section>
  ) : null;
}
