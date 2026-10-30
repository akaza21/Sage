import { rejectedGitCommand } from "../domain/gitMutation";
import type { StateCreator } from "zustand";

import type {
  GitProcessState,
  GitRepositoryCheckoutResult,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { idleAtEpoch, matchesProjectIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";

export interface RepositoryCheckout {
  rootId: string;
  branch: string;
}

export interface RepositoryCheckoutError extends RepositoryCheckout {
  kind: "failed" | "effectUnknown" | "containment";
  message: string;
  processState?: GitProcessState;
}

export interface RepositoryCheckoutSlice {
  repositoryCheckout: RepositoryCheckout | null;
  repositoryCheckoutError: RepositoryCheckoutError | null;
  checkoutLocalBranch: (rootId: string, branch: string) => Promise<boolean>;
  createLocalBranch: (rootId: string, branch: string) => Promise<boolean>;
  deleteLocalBranch: (rootId: string, branch: string) => Promise<boolean>;
  renameLocalBranch: (
    rootId: string,
    from: string,
    to: string,
  ) => Promise<boolean>;
  clearRepositoryCheckoutError: (rootId: string) => void;
}

async function runLocalBranchCommand(
  get: () => WorkbenchState,
  set: (
    partial:
      | Partial<WorkbenchState>
      | ((state: WorkbenchState) => Partial<WorkbenchState>),
  ) => void,
  rootId: string,
  branch: string,
  invoke: (
    projectId: string,
    transitionEpoch: number,
  ) => Promise<GitRepositoryCheckoutResult>,
): Promise<boolean> {
  const projectId = get().project?.id;
  const generation = get().projectGeneration;
  const transitionEpoch = get().nativeTransitionEpoch;
  if (
    !projectId ||
    !branch ||
    get().projectTransitioning ||
    get().repositoryRefreshing ||
    get().repositoryMutation ||
    get().repositoryCommit ||
    get().repositoryCheckout ||
    get().repositoryRemote ||
    !get().project?.roots.some((candidate) => candidate.id === rootId)
  ) {
    return false;
  }
  const checkout: RepositoryCheckout = { rootId, branch };
  set({ repositoryCheckout: checkout, repositoryCheckoutError: null });
  try {
    const result = await invoke(projectId, transitionEpoch);
    if (
      get().repositoryCheckout !== checkout ||
      !idleAtEpoch(get(), transitionEpoch) ||
      result.projectId !== projectId ||
      result.rootId !== rootId ||
      result.transitionEpoch !== transitionEpoch ||
      !matchesProjectIntent(get(), generation, projectId) ||
      !get().project?.roots.some((candidate) => candidate.id === rootId)
    ) {
      if (get().repositoryCheckout === checkout) {
        set({ repositoryCheckout: null });
      }
      return false;
    }
    set({
      repositories: result.repositories,
      repositoryCheckout: null,
      repositoryCheckoutError: result.error
        ? { rootId, branch, ...result.error }
        : null,
    });
    return result.error === null;
  } catch (error) {
    if (
      get().repositoryCheckout === checkout &&
      idleAtEpoch(get(), transitionEpoch) &&
      matchesProjectIntent(get(), generation, projectId)
    ) {
      set({
        repositoryCheckout: null,
        repositoryCheckoutError: {
          rootId,
          branch,
          ...rejectedGitCommand(error),
        },
      });
    } else if (get().repositoryCheckout === checkout) {
      set({ repositoryCheckout: null });
    }
    return false;
  }
}

export const createRepositoryCheckoutSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  RepositoryCheckoutSlice
> = (set, get) => ({
  repositoryCheckout: null,
  repositoryCheckoutError: null,
  clearRepositoryCheckoutError: (rootId) => {
    if (get().repositoryCheckoutError?.rootId === rootId) {
      set({ repositoryCheckoutError: null });
    }
  },
  checkoutLocalBranch: (rootId, branch) =>
    runLocalBranchCommand(get, set, rootId, branch, (projectId, epoch) =>
      desktop.checkoutGitLocalBranch({ projectId, rootId, branch }, epoch),
    ),
  createLocalBranch: (rootId, branch) =>
    runLocalBranchCommand(get, set, rootId, branch, (projectId, epoch) =>
      desktop.createGitLocalBranch({ projectId, rootId, branch }, epoch),
    ),
  deleteLocalBranch: (rootId, branch) =>
    runLocalBranchCommand(get, set, rootId, branch, (projectId, epoch) =>
      desktop.deleteGitLocalBranch({ projectId, rootId, branch }, epoch),
    ),
  renameLocalBranch: (rootId, from, to) =>
    !to
      ? Promise.resolve(false)
      : runLocalBranchCommand(get, set, rootId, from, (projectId, epoch) =>
          desktop.renameGitLocalBranch({ projectId, rootId, from, to }, epoch),
        ),
});
