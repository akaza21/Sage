import type { StateCreator } from "zustand";

import type {
  GitProcessState,
  GitRepositoryCheckoutResult,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { runRepositoryCommand } from "./runRepositoryCommand";
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
  if (!branch) return false;
  return runRepositoryCommand(
    get,
    set,
    "repositoryCheckout",
    { rootId, branch },
    invoke,
  );
}

export const createRepositoryCheckoutSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  RepositoryCheckoutSlice
> = (set, get) => ({
  repositoryCheckout: null,
  repositoryCheckoutError: null,
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
