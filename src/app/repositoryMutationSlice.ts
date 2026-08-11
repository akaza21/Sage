import type { StateCreator } from "zustand";

import { sameResource } from "../domain/resourceIdentity";
import { isConflictedChange, isSafeDiscardChange } from "../domain/gitMutation";
import type {
  GitFileMutationAction,
  GitProcessState,
  ResourceRef,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { matchesResourceIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";

export interface RepositoryMutation {
  resource: ResourceRef;
  action: GitFileMutationAction;
}

export interface RepositoryMutationError extends RepositoryMutation {
  kind: "failed" | "effectUnknown" | "containment";
  message: string;
  processState?: GitProcessState;
}

export interface RepositoryMutationSlice {
  repositoryMutation: RepositoryMutation | null;
  repositoryMutationError: RepositoryMutationError | null;
  mutateRepositoryFile: (
    resource: ResourceRef,
    action: GitFileMutationAction,
    discardConfirmed?: boolean,
  ) => Promise<boolean>;
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function rejectedMutation(error: unknown) {
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

function eligibleMutation(
  state: WorkbenchState,
  resource: ResourceRef,
  action: GitFileMutationAction,
  discardConfirmed: boolean,
) {
  const change = state.repositories.roots
    .find((root) => root.rootId === resource.rootId)
    ?.changes.find((candidate) => sameResource(candidate.resource, resource));
  if (!change || isConflictedChange(change)) {
    return false;
  }
  if (action === "discard") {
    return discardConfirmed && isSafeDiscardChange(change);
  }
  if (action === "stage") {
    return (
      change.worktreeStatus !== "unchanged" &&
      change.worktreeStatus !== "unknown"
    );
  }
  return change.indexStatus !== "unchanged" && change.indexStatus !== "unknown";
}

export const createRepositoryMutationSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  RepositoryMutationSlice
> = (set, get) => ({
  repositoryMutation: null,
  repositoryMutationError: null,
  mutateRepositoryFile: async (resource, action, discardConfirmed = false) => {
    const generation = get().projectGeneration;
    const projectId = get().project?.id;
    const transitionEpoch = get().nativeTransitionEpoch;
    if (
      !projectId ||
      get().projectTransitioning ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight ||
      get().repositoryRefreshing ||
      get().repositoryCommit ||
      get().repositoryMutation ||
      !matchesResourceIntent(get(), generation, projectId, resource) ||
      !eligibleMutation(get(), resource, action, discardConfirmed)
    ) {
      return false;
    }
    const mutation: RepositoryMutation = { resource, action };
    set({ repositoryMutation: mutation, repositoryMutationError: null });
    try {
      const result = await desktop.mutateGitFile(
        { projectId, resource, action, discardConfirmed },
        transitionEpoch,
      );
      if (
        get().repositoryMutation !== mutation ||
        get().projectTransitioning ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        result.projectId !== projectId ||
        result.transitionEpoch !== transitionEpoch ||
        !matchesResourceIntent(get(), generation, projectId, resource)
      ) {
        if (get().repositoryMutation === mutation) {
          set({ repositoryMutation: null });
        }
        return false;
      }
      set({
        repositories: result.repositories,
        repositoryMutation: null,
        repositoryMutationError: result.error
          ? { ...mutation, ...result.error }
          : null,
      });
      return result.error === null;
    } catch (error) {
      if (
        get().repositoryMutation === mutation &&
        !get().projectTransitioning &&
        get().nativeTransitionEpoch === transitionEpoch &&
        matchesResourceIntent(get(), generation, projectId, resource)
      ) {
        set({
          repositoryMutation: null,
          repositoryMutationError: {
            ...mutation,
            ...rejectedMutation(error),
          },
        });
      } else if (get().repositoryMutation === mutation) {
        set({ repositoryMutation: null });
      }
      return false;
    }
  },
});
