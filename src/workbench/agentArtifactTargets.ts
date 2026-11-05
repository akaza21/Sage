import type { AgentArtifactTarget } from "../app/agentArtifactFollowStore";
import type { WorkbenchTab } from "../domain/workbench";
import { useMemo } from "react";

function agentResourceTargets(
  artifacts: readonly WorkbenchTab[],
  previewAvailable: boolean,
): AgentArtifactTarget[] {
  return [
    ...artifacts.map((tab) => ({ kind: "tab" as const, tabId: tab.id })),
    ...(previewAvailable ? [{ kind: "preview" as const }] : []),
  ];
}

export function useAgentArtifactTargets(
  artifacts: readonly WorkbenchTab[],
  previewAvailable: boolean,
  subagents: boolean,
  changes: boolean,
) {
  return useMemo<[AgentArtifactTarget[], AgentArtifactTarget[]]>(() => {
    const resources = agentResourceTargets(artifacts, previewAvailable);
    return [
      resources,
      [
        ...resources,
        ...(subagents ? [{ kind: "subagents" as const }] : []),
        ...(changes ? [{ kind: "changes" as const }] : []),
      ],
    ];
  }, [artifacts, previewAvailable, subagents, changes]);
}

export function defaultAgentArtifactTarget(
  artifacts: readonly WorkbenchTab[],
  activeTabId: string | null,
  previewOpen: boolean,
): AgentArtifactTarget | null {
  const tab =
    artifacts.find(({ id }) => id === activeTabId) ??
    (previewOpen ? undefined : artifacts.at(-1));
  return tab
    ? { kind: "tab", tabId: tab.id }
    : previewOpen
      ? { kind: "preview" }
      : null;
}
