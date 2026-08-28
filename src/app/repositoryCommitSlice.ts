import type { StateCreator } from "zustand";

import {
  canAbortGitSequence,
  canCommitRepository,
  canContinueCherryPick,
  canContinueRebase,
  canContinueRevert,
  normalizeGitCommitMessage,
} from "../domain/gitCommit";
import type {
  GitProcessState,
  GitRepositoryCheckoutResult,
  RootRepositoryStatus,
} from "../domain/workbench";
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
  abortInProgress: (rootId: string) => Promise<boolean>;
  commitRepository: (rootId: string, message: string) => Promise<boolean>;
  continueCherryPick: (rootId: string) => Promise<boolean>;
  continueRebase: (rootId: string) => Promise<boolean>;
  continueRevert: (rootId: string) => Promise<boolean>;
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

async function runLockedSequenceCommand(
  get: () => WorkbenchState,
  set: (
    partial:
      | Partial<WorkbenchState>
      | ((state: WorkbenchState) => Partial<WorkbenchState>),
  ) => void,
  rootId: string,
  eligible: (root: RootRepositoryStatus) => boolean,
  invoke: (
    projectId: string,
    transitionEpoch: number,
  ) => Promise<GitRepositoryCheckoutResult>,
): Promise<boolean> {
  const generation = get().projectGeneration;
  const projectId = get().project?.id;
  const transitionEpoch = get().nativeTransitionEpoch;
  const root = get().repositories.roots.find(
    (candidate) => candidate.rootId === rootId,
  );
  if (
    !projectId ||
    !root ||
    get().projectTransitioning ||
    get().repositoryRefreshing ||
    get().repositoryMutation ||
    get().repositoryCommit ||
    get().repositoryCheckout ||
    get().repositoryRemote ||
    !eligible(root) ||
    !get().project?.roots.some((candidate) => candidate.id === rootId)
  ) {
    return false;
  }
  const commit: RepositoryCommit = { rootId };
  set({ repositoryCommit: commit, repositoryCommitError: null });
  try {
    const result = await invoke(projectId, transitionEpoch);
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
      repositoryCommitError: result.error ? { rootId, ...result.error } : null,
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
  abortInProgress: (rootId) =>
    runLockedSequenceCommand(
      get,
      set,
      rootId,
      canAbortGitSequence,
      (projectId, epoch) =>
        desktop.abortGitInProgress({ projectId, rootId }, epoch),
    ),
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
      get().repositoryCheckout ||
      get().repositoryRemote ||
      !canCommitRepository(root) ||
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
  continueCherryPick: (rootId) =>
    runLockedSequenceCommand(
      get,
      set,
      rootId,
      canContinueCherryPick,
      (projectId, epoch) =>
        desktop.continueGitCherryPick({ projectId, rootId }, epoch),
    ),
  continueRebase: (rootId) =>
    runLockedSequenceCommand(
      get,
      set,
      rootId,
      canContinueRebase,
      (projectId, epoch) =>
        desktop.continueGitRebase({ projectId, rootId }, epoch),
    ),
  continueRevert: (rootId) =>
    runLockedSequenceCommand(
      get,
      set,
      rootId,
      canContinueRevert,
      (projectId, epoch) =>
        desktop.continueGitRevert({ projectId, rootId }, epoch),
    ),
});
