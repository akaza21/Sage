import type { StateCreator } from "zustand";

import {
  hasStagedChanges,
  normalizeGitCommitMessage,
} from "../domain/gitCommit";
import type { GitProcessState } from "../domain/workbench";
import { desktop } from "../services/desktop";
import { matchesProjectIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";

export interface RepositoryCommit {
  rootId: string;
}

export interface RepositoryCommitError extends RepositoryCommit {
  kind: "failed" | "effectUnknown" | "containment";
  message: string;
  processState?: GitProcessState;
}

export interface RepositoryCommitSlice {
  repositoryCommit: RepositoryCommit | null;
  repositoryCommitError: RepositoryCommitError | null;
  commitRepository: (rootId: string, message: string) => Promise<boolean>;
  clearRepositoryCommitError: (rootId: string) => void;
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function rejectedCommit(error: unknown) {
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
      } as const;
    }
    return { kind: error.kind, message: error.message } as const;
  }
  return { kind: "failed", message: errorMessage(error) } as const;
}

export const createRepositoryCommitSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  RepositoryCommitSlice
> = (set, get) => ({
  repositoryCommit: null,
  repositoryCommitError: null,
  clearRepositoryCommitError: (rootId) => {
    const error = get().repositoryCommitError;
    if (error?.rootId === rootId && error.kind !== "effectUnknown") {
      set({ repositoryCommitError: null });
    }
  },
  commitRepository: async (rootId, message) => {
    const generation = get().projectGeneration;
    const projectId = get().project?.id;
    const transitionEpoch = get().nativeTransitionEpoch;
    const root = get().repositories.roots.find(
      (candidate) => candidate.rootId === rootId,
    );
    let normalizedMessage: string;
    try {
      normalizedMessage = normalizeGitCommitMessage(message);
    } catch (error) {
      set({
        repositoryCommitError: {
          rootId,
          kind: "failed",
          message: errorMessage(error),
        },
      });
      return false;
    }
    if (
      !projectId ||
      !root ||
      get().projectTransitioning ||
      get().repositoryRefreshing ||
      get().repositoryMutation ||
      get().repositoryCommit ||
      !hasStagedChanges(root) ||
      !get().project?.roots.some((candidate) => candidate.id === rootId)
    ) {
      return false;
    }
    const commit: RepositoryCommit = { rootId };
    set({ repositoryCommit: commit, repositoryCommitError: null });
    try {
      const result = await desktop.commitGitRepository(
        { projectId, rootId, message: normalizedMessage },
        transitionEpoch,
      );
      if (
        get().repositoryCommit !== commit ||
        get().projectTransitioning ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        result.projectId !== projectId ||
        result.rootId !== rootId ||
        result.transitionEpoch !== transitionEpoch ||
        !matchesProjectIntent(get(), generation, projectId) ||
        !get().project?.roots.some((candidate) => candidate.id === rootId)
      ) {
        if (get().repositoryCommit === commit) {
          set({ repositoryCommit: null });
        }
        return false;
      }
      set({
        repositories: result.repositories,
        repositoryCommit: null,
        repositoryCommitError: result.error
          ? { rootId, ...result.error }
          : null,
      });
      return result.error === null;
    } catch (error) {
      if (
        get().repositoryCommit === commit &&
        !get().projectTransitioning &&
        get().nativeTransitionEpoch === transitionEpoch &&
        matchesProjectIntent(get(), generation, projectId)
      ) {
        set({
          repositoryCommit: null,
          repositoryCommitError: { rootId, ...rejectedCommit(error) },
        });
      } else if (get().repositoryCommit === commit) {
        set({ repositoryCommit: null });
      }
      return false;
    }
  },
});
