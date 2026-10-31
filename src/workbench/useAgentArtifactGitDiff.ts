import { useEffect, useState } from "react";
import { useWorkbenchStore, type ResourceState } from "../app/store";
import type { FileDiff, WorkbenchTab } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useAgentChangesScopes } from "./agentChangesStateScopes";

export function useAgentArtifactGitDiff(tab: WorkbenchTab) {
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const scope = useAgentChangesScopes(
    (state) =>
      state.gitScopes.find(
        (entry) => entry.tabId === tab.id && entry.generation === generation,
      )?.scope,
  );
  const [revision, refresh] = useState(0);
  const [result, setResult] = useState<{
    tab: WorkbenchTab;
    scope: string;
    generation: number;
    revision: number;
    diff: ResourceState<FileDiff>;
  } | null>(null);
  useEffect(() => {
    if (!scope || !tab.resource || tab.diffRef) return;
    let active = true;
    const resource = tab.resource;
    const initial = useWorkbenchStore.getState();
    const current = () =>
      active &&
      !useWorkbenchStore.getState().projectTransitioning &&
      useWorkbenchStore.getState().projectGeneration === generation &&
      useWorkbenchStore.getState().tabs.includes(tab);
    if (
      initial.projectTransitioning ||
      !initial.project?.roots.some((root) => root.id === resource.rootId)
    )
      return;
    void getWorkbenchPorts()
      .repository.getFileDiff(resource, scope)
      .then(
        (value) => {
          if (current())
            setResult({
              tab,
              scope,
              generation,
              revision,
              diff: { phase: "ready", value },
            });
        },
        (cause) => {
          if (current())
            setResult({
              tab,
              scope,
              generation,
              revision,
              diff: { phase: "error", message: String(cause).slice(0, 512) },
            });
        },
      );
    return () => {
      active = false;
    };
  }, [tab, scope, generation, revision]);
  return {
    scope,
    diff: scope
      ? result?.tab === tab &&
        result.scope === scope &&
        result.generation === generation &&
        result.revision === revision
        ? result.diff
        : { phase: "loading" as const }
      : undefined,
    refresh: () => refresh((value) => value + 1),
  };
}
