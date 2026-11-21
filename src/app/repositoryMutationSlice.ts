import type { StateCreator } from "zustand";

import { sameResource } from "../domain/resourceIdentity";
import {
  canResolveConflictedChange,
  isConflictedChange,
  isSafeDiscardChange,
} from "../domain/gitMutation";
import type {
  GitFileMutationAction,
  GitProcessState,
  ResourceRef,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { runRepositoryCommand } from "./runRepositoryCommand";
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
  mutateRepositoryPaths: (
    resources: ResourceRef[],
    action: "stage" | "unstage" | "discard",
    discardConfirmed?: boolean,
  ) => Promise<boolean>;
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
  if (!change) {
    return false;
  }
  if (canResolveConflictedChange(change, action)) {
    return true;
  }
  if (isConflictedChange(change)) {
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
  mutateRepositoryPaths: async (
    resources,
    action,
    discardConfirmed = false,
  ) => {
    const state = get();
    const resource = resources[0];
    const root = state.repositories.roots.find(
      (root) => root.rootId === resource?.rootId,
    );
    if (
      !resource ||
      resources.length > 300 ||
      root?.state !== "repository" ||
      root.truncated ||
      root.changes.length > 300 ||
      state.fileRenameInFlight ||
      state.resourceTrashInFlight ||
      resources.some(
        (candidate) =>
          candidate.rootId !== resource.rootId ||
          !eligibleMutation(state, candidate, action, discardConfirmed),
      )
    )
      return false;
    return runRepositoryCommand(
      get,
      set,
      "repositoryMutation",
      { resource, action },
      (projectId, epoch) => {
        const request = { projectId, resources, discardConfirmed };
        if (action === "stage") return desktop.stageGitPaths(request, epoch);
        if (action === "unstage")
          return desktop.unstageGitPaths(request, epoch);
        return desktop.discardGitPaths(request, epoch);
      },
    );
  },
  mutateRepositoryFile: async (resource, action, discardConfirmed = false) => {
    if (
      get().fileRenameInFlight ||
      get().resourceTrashInFlight ||
      !eligibleMutation(get(), resource, action, discardConfirmed)
    ) {
      return false;
    }
    return runRepositoryCommand(
      get,
      set,
      "repositoryMutation",
      { resource, action },
      (projectId, transitionEpoch) =>
        desktop.mutateGitFile(
          { projectId, resource, action, discardConfirmed },
          transitionEpoch,
        ),
    );
  },
});
