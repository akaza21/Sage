import { useSubagentPaneStore } from "../app/subagentPaneStore";
import type { projectTaskResources } from "./agentArtifactTabPresentation";

export function SubagentResourceRows({
  agents,
  onSelect,
}: {
  agents: ReturnType<typeof projectTaskResources>["recentAgents"];
  onSelect: () => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list?.closest('[role="dialog"]') === document.activeElement) {
      list?.querySelector("button")?.focus();
    }
  }, []);
  return (
    <div ref={listRef} className="agent-task-header__agent-list">
      {agents.map((agent) => {
        const target = agent.target;
        const Row = target ? "button" : "p";
        return (
          <Row
            className="agent-task-header__agent agent-task-header__resource-row"
            key={agent.id}
            {...(target
              ? {
                  type: "button" as const,
                  "aria-label": `Open subagent ${agent.label}`,
                  onClick: () => {
                    useSubagentPaneStore.getState().show(target);
                    onSelect();
                  },
                }
              : {})}
          >
            <span>{agent.label}</span>
            <span>
              {agent.status === "starting"
                ? "Starting when observed"
                : agent.status === "working"
                  ? "Working when observed"
                  : agent.status}
            </span>
          </Row>
        );
      })}
    </div>
  );
}
import { useLayoutEffect, useRef } from "react";
