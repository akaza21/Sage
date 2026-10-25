import type { StateCreator } from "zustand";

import type { GitProcessState } from "../domain/workbench";
import { desktop } from "../services/desktop";
import { idleAtEpoch, matchesProjectIntent } from "./projectIntent";
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
  clearRepositoryRemoteError: (rootId: string) => void;
}

function rejectedRemote(
  error: unknown,
): Omit<RepositoryRemoteError, "rootId" | "remote" | "action"> {
  if (
    typeof error === "object" &&
    error !== null &&
    "kind" in error &&
    "message" in error &&
    (error.kind === "failed" || error.kind === "containment") &&
    typeof error.message === "string"
  ) {
    if (
      error.kind === "containment" &&
      "processState" in error &&
      (error.processState === "terminatedUnreaped" ||
        error.processState === "potentiallyRunning")
    ) {
      return {
        kind: error.kind,
        message: error.message,
        processState: error.processState,
      };
    }
    return { kind: error.kind, message: error.message };
  }
  return { kind: "failed", message: errorMessage(error) };
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
    const projectId = get().project?.id;
    const generation = get().projectGeneration;
    const transitionEpoch = get().nativeTransitionEpoch;
    if (
      !projectId ||
      (action !== "pull" && !remote) ||
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
    const transport: RepositoryRemote = { rootId, remote, action };
    set({ repositoryRemote: transport, repositoryRemoteError: null });
    try {
      const result =
        action === "pull"
          ? await desktop.pullGitRepository(
              { projectId, rootId },
              transitionEpoch,
            )
          : await (
              action === "fetch"
                ? desktop.fetchGitRemote
                : desktop.pushGitRemote
            )({ projectId, rootId, remote: remote! }, transitionEpoch);
      if (
        get().repositoryRemote !== transport ||
        !idleAtEpoch(get(), transitionEpoch) ||
        result.projectId !== projectId ||
        result.rootId !== rootId ||
        result.transitionEpoch !== transitionEpoch ||
        !matchesProjectIntent(get(), generation, projectId) ||
        !get().project?.roots.some((candidate) => candidate.id === rootId)
      ) {
        if (get().repositoryRemote === transport) {
          set({ repositoryRemote: null });
        }
        return false;
      }
      set({
        repositories: result.repositories,
        repositoryRemote: null,
        repositoryRemoteError: result.error
          ? { rootId, remote, action, ...result.error }
          : null,
      });
      return result.error === null;
    } catch (error) {
      if (
        get().repositoryRemote === transport &&
        idleAtEpoch(get(), transitionEpoch) &&
        matchesProjectIntent(get(), generation, projectId)
      ) {
        set({
          repositoryRemote: null,
          repositoryRemoteError: {
            rootId,
            remote,
            action,
            ...rejectedRemote(error),
          },
        });
      } else if (get().repositoryRemote === transport) {
        set({ repositoryRemote: null });
      }
      return false;
    }
  }

  return {
    repositoryRemote: null,
    repositoryRemoteError: null,
    clearRepositoryRemoteError: (rootId) => {
      if (get().repositoryRemoteError?.rootId === rootId) {
        set({ repositoryRemoteError: null });
      }
    },
    fetchRemote: (rootId, remote) => transportRemote(rootId, remote, "fetch"),
    pullRepository: (rootId) => transportRemote(rootId, null, "pull"),
    pushRemote: (rootId, remote) => transportRemote(rootId, remote, "push"),
  };
};
import { errorMessage } from "../domain/errors";
