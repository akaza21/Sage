import type { ReactNode } from "react";
import type { AcpChatPage } from "../domain/acpProjection";
import { liveAgent, type RecentAgent } from "./agentArtifactTabPresentation";
import { SubagentResourceRows } from "./SubagentResourceRows";
import { useChildEpoch, useSettledSubagents } from "./subagentLiveness";

export function AgentTaskSubagentSummary({
  page,
  projectGeneration,
  recentAgents,
  counts,
  onSelect,
}: {
  page: AcpChatPage;
  projectGeneration: string;
  recentAgents: readonly RecentAgent[];
  counts: (agents: readonly RecentAgent[], glyphs: ReactNode) => ReactNode;
  onSelect: () => void;
}) {
  const agents = useSettledSubagents(
    page,
    projectGeneration,
    recentAgents,
    useChildEpoch(page),
  );
  const working = agents.filter((agent) => liveAgent(agent.status)).length;
  return (
    <section
      aria-label="Subagents"
      title="The latest 128 subagents found in the latest 256 items of the latest 128 turns"
    >
      <strong role="heading" aria-level={3}>
        Subagents
      </strong>
      {counts(
        agents,
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
      <SubagentResourceRows agents={agents} onSelect={onSelect} />
    </section>
  );
}
