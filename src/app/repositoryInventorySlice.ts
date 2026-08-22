import type { StateCreator } from "zustand";

import {
  isRootRepository,
  rootRepositoryStatus,
  sameRepository,
} from "../domain/repositoryInventory";
import type { RepositoryRef, RootRepositoryStatus } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import type { WorkbenchState } from "./store";

export type RepositoryInventoryPhase = "idle" | "loading" | "ready" | "error";
export type RepositoryStatusPhase = "idle" | "loading" | "ready" | "error";

export interface RepositoryInventorySlice {
  repositoryInventoryPhase: RepositoryInventoryPhase;
  repositoryInventoryError: string | null;
  repositoryRefs: RepositoryRef[];
  repositoryDiscoveryTruncated: boolean;
  repositoryInventoryRequestGeneration: number;
  selectedRepository: RepositoryRef | null;
  selectedRepositoryStatus: RootRepositoryStatus | null;
  repositoryStatusPhase: RepositoryStatusPhase;
  repositoryStatusError: string | null;
  repositoryStatusRequestGeneration: number;
  loadRepositoryInventory: () => Promise<void>;
  selectRepository: (repository: RepositoryRef) => Promise<void>;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function canLoad(state: WorkbenchState): boolean {
  return !(
    state.projectTransitioning ||
    state.fileRenameInFlight ||
    state.resourceTrashInFlight
  );
}

export const createRepositoryInventorySlice: StateCreator<
  WorkbenchState,
  [],
  [],
  RepositoryInventorySlice
> = (set, get) => ({
  repositoryInventoryPhase: "idle",
  repositoryInventoryError: null,
  repositoryRefs: [],
  repositoryDiscoveryTruncated: false,
  repositoryInventoryRequestGeneration: 0,
  selectedRepository: null,
  selectedRepositoryStatus: null,
  repositoryStatusPhase: "idle",
  repositoryStatusError: null,
  repositoryStatusRequestGeneration: 0,

  loadRepositoryInventory: async () => {
    if (!canLoad(get())) return;
    const projectId = get().project?.id;
    if (!projectId) return;
    const projectGeneration = get().projectGeneration;
    const expectedTransitionEpoch = get().nativeTransitionEpoch;
    const requestGeneration = nextProjectGeneration(
      get().repositoryInventoryRequestGeneration,
    );
    set({
      repositoryInventoryPhase: "loading",
      repositoryInventoryError: null,
      repositoryInventoryRequestGeneration: requestGeneration,
    });
    try {
      const result =
        await getWorkbenchPorts().repository.discoverProjectRepositories({
          projectId,
          projectGeneration,
          expectedTransitionEpoch,
        });
      if (
        !matchesProjectIntent(get(), projectGeneration, projectId) ||
        get().nativeTransitionEpoch !== expectedTransitionEpoch ||
        get().repositoryInventoryRequestGeneration !== requestGeneration ||
        result.projectId !== projectId ||
        result.transitionEpoch !== expectedTransitionEpoch
      ) {
        return;
      }
      const selected = get().selectedRepository;
      const nextSelected =
        selected && result.refs.some((ref) => sameRepository(ref, selected))
          ? selected
          : (result.refs[0] ?? null);
      set({
        repositoryInventoryPhase: "ready",
        repositoryRefs: result.refs,
        repositoryDiscoveryTruncated: result.truncated,
        selectedRepository: nextSelected,
        selectedRepositoryStatus: null,
        repositoryStatusError: null,
        repositoryStatusPhase: nextSelected ? "loading" : "idle",
      });
      if (nextSelected) await get().selectRepository(nextSelected);
    } catch (error) {
      if (
        matchesProjectIntent(get(), projectGeneration, projectId) &&
        get().nativeTransitionEpoch === expectedTransitionEpoch &&
        get().repositoryInventoryRequestGeneration === requestGeneration
      ) {
        set({
          repositoryInventoryPhase: "error",
          repositoryInventoryError: errorMessage(error),
          repositoryRefs: [],
          repositoryDiscoveryTruncated: false,
          selectedRepository: null,
          selectedRepositoryStatus: null,
          repositoryStatusPhase: "idle",
        });
      }
    }
  },

  selectRepository: async (repository) => {
    if (!canLoad(get())) return;
    const projectId = get().project?.id;
    if (!projectId) return;
    const projectGeneration = get().projectGeneration;
    const expectedTransitionEpoch = get().nativeTransitionEpoch;
    const requestGeneration = nextProjectGeneration(
      get().repositoryStatusRequestGeneration,
    );
    const rootStatus = rootRepositoryStatus(
      repository,
      get().repositories.roots,
    );
    set({
      selectedRepository: repository,
      selectedRepositoryStatus: rootStatus,
      repositoryStatusPhase: isRootRepository(repository) ? "ready" : "loading",
      repositoryStatusError: null,
      repositoryStatusRequestGeneration: requestGeneration,
    });
    if (isRootRepository(repository)) return;
    try {
      const result =
        await getWorkbenchPorts().repository.getNestedRepositoryStatus({
          projectId,
          projectGeneration,
          expectedTransitionEpoch,
          ref: repository,
        });
      if (
        !matchesProjectIntent(get(), projectGeneration, projectId) ||
        get().nativeTransitionEpoch !== expectedTransitionEpoch ||
        get().repositoryStatusRequestGeneration !== requestGeneration ||
        !sameRepository(get().selectedRepository, repository) ||
        result.projectId !== projectId ||
        result.transitionEpoch !== expectedTransitionEpoch ||
        !sameRepository(result.ref, repository)
      ) {
        return;
      }
      set({
        selectedRepositoryStatus: result.status,
        repositoryStatusPhase: "ready",
        repositoryStatusError: null,
      });
    } catch (error) {
      if (
        matchesProjectIntent(get(), projectGeneration, projectId) &&
        get().nativeTransitionEpoch === expectedTransitionEpoch &&
        get().repositoryStatusRequestGeneration === requestGeneration &&
        sameRepository(get().selectedRepository, repository)
      ) {
        set({
          selectedRepositoryStatus: null,
          repositoryStatusPhase: "error",
          repositoryStatusError: errorMessage(error),
        });
      }
    }
  },
});
