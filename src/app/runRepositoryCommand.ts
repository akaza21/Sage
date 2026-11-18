import type { StoreApi } from "zustand";
import { rejectedGitCommand } from "../domain/gitMutation";
import type { GitFileMutationResult } from "../domain/workbench";
import {
  idleAtEpoch,
  matchesProjectIntent,
  repositoryBusy,
} from "./projectIntent";
import type { WorkbenchState } from "./store";

type OperationSlot =
  | "repositoryCheckout"
  | "repositoryCommit"
  | "repositoryRemote"
  | "repositoryMutation";

export async function runRepositoryCommand<K extends OperationSlot>(
  get: () => WorkbenchState,
  set: StoreApi<WorkbenchState>["setState"],
  slot: K,
  operation: NonNullable<WorkbenchState[K]>,
  invoke: (
    projectId: string,
    epoch: number,
  ) => Promise<GitFileMutationResult & { rootId?: string }>,
): Promise<boolean> {
  const initial = get();
  const projectId = initial.project?.id;
  const generation = initial.projectGeneration;
  const epoch = initial.nativeTransitionEpoch;
  const rootId =
    "resource" in operation ? operation.resource.rootId : operation.rootId;
  const fileMutation = slot === "repositoryMutation";
  const rootAdmitted = () =>
    get().project?.roots.some((root) => root.id === rootId);
  const errorSlot = `${slot}Error` as const;
  if (!projectId || repositoryBusy(initial) || !rootAdmitted()) return false;
  set({ [slot]: operation, [errorSlot]: null });
  const owned = () => get()[slot] === operation;
  const current = () =>
    owned() &&
    idleAtEpoch(get(), epoch) &&
    matchesProjectIntent(get(), generation, projectId) &&
    (!fileMutation || rootAdmitted());
  try {
    const result = await invoke(projectId, epoch);
    if (
      !current() ||
      result.projectId !== projectId ||
      (!fileMutation && result.rootId !== rootId) ||
      result.transitionEpoch !== epoch ||
      !rootAdmitted()
    ) {
      if (owned()) set({ [slot]: null });
      return false;
    }
    set({
      repositories: result.repositories,
      [slot]: null,
      [errorSlot]: result.error ? { ...operation, ...result.error } : null,
    });
    return result.error === null;
  } catch (error) {
    if (current())
      set({
        [slot]: null,
        [errorSlot]: { ...operation, ...rejectedGitCommand(error) },
      });
    else if (owned()) set({ [slot]: null });
    return false;
  }
}
