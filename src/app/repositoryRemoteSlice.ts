import type { StateCreator } from "zustand";

import type { GitProcessState } from "../domain/workbench";
import { desktop } from "../services/desktop";
import { runRepositoryCommand } from "./runRepositoryCommand";
import type { WorkbenchState } from "./store";

export type RepositoryRemoteAction = "fetch" | "push" | "pull";

export interface RepositoryRemote {
  rootId: string;
  remote: string | null;
  action: RepositoryRemoteAction;
}

export interface RepositoryRemoteError extends RepositoryRemote {
  kind: "failed" | "effectUnknown" | "containment";
  message: string;
  processState?: GitProcessState;
}

export interface RepositoryRemoteSlice {
  repositoryRemote: RepositoryRemote | null;
  repositoryRemoteError: RepositoryRemoteError | null;
  fetchRemote: (rootId: string, remote: string) => Promise<boolean>;
  pullRepository: (rootId: string) => Promise<boolean>;
  pushRemote: (rootId: string, remote: string) => Promise<boolean>;
}

export const createRepositoryRemoteSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  RepositoryRemoteSlice
> = (set, get) => {
  async function transportRemote(
    rootId: string,
    remote: string | null,
    action: RepositoryRemoteAction,
  ): Promise<boolean> {
    if (action !== "pull" && !remote) return false;
    return runRepositoryCommand(
      get,
      set,
      "repositoryRemote",
      { rootId, remote, action },
      (projectId, epoch) =>
        action === "pull"
          ? desktop.pullGitRepository({ projectId, rootId }, epoch)
          : (action === "fetch"
              ? desktop.fetchGitRemote
              : desktop.pushGitRemote)(
              { projectId, rootId, remote: remote! },
              epoch,
            ),
    );
  }

  return {
    repositoryRemote: null,
    repositoryRemoteError: null,
    fetchRemote: (rootId, remote) => transportRemote(rootId, remote, "fetch"),
    pullRepository: (rootId) => transportRemote(rootId, null, "pull"),
    pushRemote: (rootId, remote) => transportRemote(rootId, remote, "push"),
  };
};
