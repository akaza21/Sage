import { create } from "zustand";
import type { WorkbenchTab } from "../domain/workbench";
import { useWorkbenchStore } from "../app/store";
const MAX_SCOPED_DIFF_TABS = 32;
export const useAgentChangesScopes = create<{
  gitScopes: readonly {
    tabId: string;
    scope: "staged" | "unstaged";
    generation: number;
  }[];
  setGitScope: (
    tab: WorkbenchTab,
    scope: "staged" | "unstaged",
    generation: number,
  ) => void;
}>((set) => ({
  gitScopes: [],
  setGitScope: (tab, scope, generation) =>
    set((state) => ({
      gitScopes: [
        ...state.gitScopes.filter((entry) => entry.tabId !== tab.id),
        { tabId: tab.id, scope, generation },
      ].slice(-MAX_SCOPED_DIFF_TABS),
    })),
}));

useWorkbenchStore.subscribe((state, previous) => {
  if (
    state.tabs === previous.tabs &&
    state.projectGeneration === previous.projectGeneration
  )
    return;
  const current = useAgentChangesScopes.getState().gitScopes;
  const retained = current.filter(
    (entry) =>
      entry.generation === state.projectGeneration &&
      state.tabs.some((tab) => tab.id === entry.tabId),
  );
  if (retained.length !== current.length)
    useAgentChangesScopes.setState({ gitScopes: retained });
});
