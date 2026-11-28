import type { StateCreator } from "zustand";

import {
  isRootRepository,
  rootRepositoryStatus,
  sameRepository,
} from "../domain/repositoryInventory";
import type {
  RepositoryRef,
  RepositorySummary,
  RootRepositoryStatus,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import type { WorkbenchState } from "./store";

export type RepositoryInventoryPhase = "idle" | "loading" | "ready" | "error";
export type RepositoryStatusPhase = "idle" | "loading" | "ready" | "error";

export interface RepositoryInventorySlice {
  repositoryInventoryPhase: RepositoryInventoryPhase;
  repositoryInventoryError: string | null;
  repositoryRefs: RepositoryRef[];
  /** Folder-of-repositories rows; empty when the root is itself a repository. */
  repositorySummaries: RepositorySummary[];
  repositoryDiscoveryTruncated: boolean;
  repositoryInventoryProjectScope: string | null;
  repositoryInventoryRequestGeneration: number;
  selectedRepository: RepositoryRef | null;
  selectedRepositoryStatus: RootRepositoryStatus | null;
  repositoryStatusPhase: RepositoryStatusPhase;
  repositoryStatusError: string | null;
  repositoryStatusRequestGeneration: number;
  /** `background` keeps the shown list and changes until fresh ones arrive. */
  loadRepositoryInventory: (options?: {
    background?: boolean;
  }) => Promise<void>;
  selectRepository: (
    repository: RepositoryRef,
    options?: { background?: boolean },
  ) => Promise<void>;
}

function canLoad(state: WorkbenchState): boolean {
  return !(
    state.projectTransitioning ||
    state.fileRenameInFlight ||
    state.resourceTrashInFlight
  );
}

function projectScope(state: WorkbenchState): string | null {
  if (!state.project) return null;
  return `${state.project.id}\0${state.projectGeneration}\0${state.nativeTransitionEpoch}`;
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
  repositorySummaries: [],
  repositoryDiscoveryTruncated: false,
  repositoryInventoryProjectScope: null,
  repositoryInventoryRequestGeneration: 0,
  selectedRepository: null,
  selectedRepositoryStatus: null,
  repositoryStatusPhase: "idle",
  repositoryStatusError: null,
  repositoryStatusRequestGeneration: 0,

  loadRepositoryInventory: async (options) => {
    const current = get();
    const projectId = current.project?.id;
    const scope = projectScope(current);
    const background =
      options?.background === true &&
      current.repositoryInventoryProjectScope === scope &&
      current.repositoryInventoryPhase === "ready";
    if (current.repositoryInventoryProjectScope !== scope) {
      set({
        repositoryInventoryPhase: "idle",
        repositoryInventoryError: null,
        repositoryRefs: [],
        repositorySummaries: [],
        repositoryDiscoveryTruncated: false,
        repositoryInventoryProjectScope: scope,
        repositoryInventoryRequestGeneration: nextProjectGeneration(
          current.repositoryInventoryRequestGeneration,
        ),
        selectedRepository: null,
        selectedRepositoryStatus: null,
        repositoryStatusPhase: "idle",
        repositoryStatusError: null,
        repositoryStatusRequestGeneration: nextProjectGeneration(
          current.repositoryStatusRequestGeneration,
        ),
      });
    }
    if (!canLoad(get())) return;
    if (!projectId) return;
    const projectGeneration = get().projectGeneration;
    const expectedTransitionEpoch = get().nativeTransitionEpoch;
    const requestGeneration = nextProjectGeneration(
      get().repositoryInventoryRequestGeneration,
    );
    set({
      repositoryInventoryPhase: background ? "ready" : "loading",
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
      const kept =
        selected !== null &&
        result.refs.some((ref) => sameRepository(ref, selected));
      const nextSelected = kept ? selected : (result.refs[0] ?? null);
      const keepStatus = background && kept;
      set({
        repositoryInventoryPhase: "ready",
        repositoryRefs: result.refs,
        repositorySummaries: result.summaries ?? [],
        repositoryDiscoveryTruncated: result.truncated,
        repositoryInventoryProjectScope: scope,
        selectedRepository: nextSelected,
        ...(keepStatus
          ? {}
          : {
              selectedRepositoryStatus: null,
              repositoryStatusError: null,
              repositoryStatusPhase: nextSelected ? "loading" : "idle",
            }),
      });
      if (nextSelected) {
        await get().selectRepository(nextSelected, { background: keepStatus });
      }
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
          repositorySummaries: [],
          repositoryDiscoveryTruncated: false,
          repositoryInventoryProjectScope: scope,
          selectedRepository: null,
          selectedRepositoryStatus: null,
          repositoryStatusPhase: "idle",
        });
      }
    }
  },

  selectRepository: async (repository, options) => {
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
    const quiet =
      options?.background === true &&
      !isRootRepository(repository) &&
      sameRepository(get().selectedRepository, repository) &&
      get().selectedRepositoryStatus !== null;
    set(
      quiet
        ? { repositoryStatusRequestGeneration: requestGeneration }
        : {
            selectedRepository: repository,
            selectedRepositoryStatus: rootStatus,
            repositoryStatusPhase: isRootRepository(repository)
              ? "ready"
              : "loading",
            repositoryStatusError: null,
            repositoryStatusRequestGeneration: requestGeneration,
          },
    );
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
import { errorMessage } from "../domain/errors";
