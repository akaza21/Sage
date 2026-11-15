import { errorMessage } from "../domain/errors";
import { resourceKey } from "../domain/resourceIdentity";
import type { DirectoryListing, ResourceRef } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  issueRequestIntent,
  matchesRequestIntent,
  matchesResourceIntent,
} from "./projectIntent";
import { boundedResourceStateUpdate } from "./resourceStateHelpers";
import type { ResourceState, WorkbenchState } from "./store";

export const MAX_CACHED_DIRECTORIES = 64;

interface DirectoryLoad {
  refreshRequested: boolean;
  completion: Promise<void>;
}

// Loading entries own their task: eviction or replacement cannot retain it.
const directoryLoads = new WeakMap<
  ResourceState<DirectoryListing>,
  DirectoryLoad
>();

export async function runLoadDirectory(
  get: () => WorkbenchState,
  set: (partial: object | ((state: WorkbenchState) => object)) => void,
  resource: ResourceRef,
  refresh = false,
): Promise<void> {
  const generation = get().projectGeneration;
  const projectId = get().project?.id;
  if (
    !projectId ||
    !matchesResourceIntent(get(), generation, projectId, resource)
  ) {
    return;
  }
  const key = resourceKey(resource);
  const requestKey = `directory:${generation}:${key}`;
  const existing = get().directoryListings[key];
  if (
    existing?.phase === "loading" &&
    get().requestGenerations[requestKey] !== undefined
  ) {
    const active = directoryLoads.get(existing);
    if (refresh && active) {
      active.refreshRequested = true;
      // Joining also keeps a notify admission alive when Explorer started
      // the read, until both that read and its trailing refresh finish.
      return active.completion;
    }
    return;
  }
  if (existing?.phase === "ready" && !refresh && !existing.value.truncated) {
    return;
  }
  let finish!: () => void;
  const active: DirectoryLoad = {
    refreshRequested: false,
    completion: new Promise<void>((resolve) => {
      finish = resolve;
    }),
  };
  try {
    do {
      active.refreshRequested = false;
      let requestGeneration = 0;
      const loading: ResourceState<DirectoryListing> = { phase: "loading" };
      directoryLoads.set(loading, active);
      set((state) => {
        const request = issueRequestIntent(
          state.requestGenerations,
          requestKey,
          state.requestIntentSequence,
        );
        requestGeneration = request.token;
        return {
          requestGenerations: request.intents,
          requestIntentSequence: request.sequence,
          directoryListings: boundedResourceStateUpdate(
            state.directoryListings,
            key,
            loading,
            MAX_CACHED_DIRECTORIES,
          ),
        };
      });
      const acceptsDirectoryResult = () => {
        const current = get();
        const projectMatches = matchesResourceIntent(
          current,
          generation,
          projectId,
          resource,
        );
        const requestMatches = matchesRequestIntent(
          current,
          requestKey,
          requestGeneration,
        );
        const currentRequestKey = `directory:${current.projectGeneration}:${key}`;
        const newerGenerationIsLoading =
          currentRequestKey !== requestKey &&
          current.requestGenerations[currentRequestKey] !== undefined;
        if (!projectMatches && requestMatches && !newerGenerationIsLoading) {
          set((state) => {
            if (
              state.requestGenerations[requestKey] !== requestGeneration ||
              state.directoryListings[key]?.phase !== "loading"
            ) {
              return {};
            }
            const directoryListings = { ...state.directoryListings };
            delete directoryListings[key];
            return { directoryListings };
          });
        }
        return (
          projectMatches &&
          requestMatches &&
          current.directoryListings[key]?.phase === "loading"
        );
      };
      try {
        const value = await getWorkbenchPorts().project.listDirectory(resource);
        if (!acceptsDirectoryResult()) {
          return;
        }
        set((state) => ({
          directoryListings: boundedResourceStateUpdate(
            state.directoryListings,
            key,
            { phase: "ready", value },
            MAX_CACHED_DIRECTORIES,
          ),
        }));
      } catch (error) {
        if (!acceptsDirectoryResult()) {
          return;
        }
        set((state) => ({
          directoryListings: boundedResourceStateUpdate(
            state.directoryListings,
            key,
            { phase: "error", message: errorMessage(error) },
            MAX_CACHED_DIRECTORIES,
          ),
        }));
      }
      if (
        !matchesResourceIntent(get(), generation, projectId, resource) ||
        !matchesRequestIntent(get(), requestKey, requestGeneration) ||
        !get().directoryListings[key]
      ) {
        return;
      }
      // Loop in the same task so an admitted load keeps its listing slot.
    } while (active.refreshRequested);
  } finally {
    finish();
  }
}
