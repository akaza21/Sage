import { diffCacheKey, isNestedRepository } from "../domain/diffIdentity";
import { sameResource } from "../domain/resourceIdentity";
import type { FileDiff, RepositoryRef, ResourceRef } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  hasTabResourceIntent,
  issueRequestIntent,
  matchesActiveTabIntent,
  matchesRequestIntent,
} from "./projectIntent";
import {
  boundedResourceStateUpdate,
  errorMessage,
} from "./resourceStateHelpers";
import type { WorkbenchState } from "./store";

const MAX_CACHED_DIFFS = 32;

export async function fetchWorkbenchFileDiff(
  resource: ResourceRef,
  repository: RepositoryRef | undefined,
  projectId: string,
  projectGeneration: number,
  expectedTransitionEpoch: number,
): Promise<FileDiff> {
  if (isNestedRepository(repository)) {
    const result =
      await getWorkbenchPorts().repository.getNestedRepositoryFileDiff({
        projectId,
        projectGeneration,
        expectedTransitionEpoch,
        ref: repository,
        resource,
      });
    return result.diff;
  }
  return getWorkbenchPorts().repository.getFileDiff(resource);
}

export async function runLoadDiff(
  get: () => WorkbenchState,
  set: (partial: object | ((state: WorkbenchState) => object)) => void,
  resource: ResourceRef,
  repository?: RepositoryRef,
): Promise<void> {
  const generation = get().projectGeneration;
  const projectId = get().project?.id;
  const active = get().tabs.find((tab) => tab.id === get().activeTabId);
  const resolvedRepository =
    repository ??
    (active?.kind === "diff" && sameResource(active.resource, resource)
      ? active.repository
      : undefined);
  if (
    !projectId ||
    !hasTabResourceIntent(get(), generation, projectId, "diff", resource)
  ) {
    return;
  }
  const key = diffCacheKey(resolvedRepository, resource);
  const requestKey = `diff:${key}`;
  const existing = get().diffs[key];
  if (existing?.phase === "loading" || existing?.phase === "ready") {
    return;
  }
  let requestGeneration = 0;
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
      diffs: boundedResourceStateUpdate(
        state.diffs,
        key,
        { phase: "loading" },
        MAX_CACHED_DIFFS,
      ),
    };
  });
  try {
    const value = await fetchWorkbenchFileDiff(
      resource,
      resolvedRepository,
      projectId,
      generation,
      get().nativeTransitionEpoch,
    );
    if (
      !matchesActiveTabIntent(get(), generation, projectId, "diff", resource) ||
      !matchesRequestIntent(get(), requestKey, requestGeneration)
    ) {
      return;
    }
    set((state) => ({
      diffs: boundedResourceStateUpdate(
        state.diffs,
        key,
        { phase: "ready", value },
        MAX_CACHED_DIFFS,
      ),
    }));
  } catch (error) {
    if (
      !matchesActiveTabIntent(get(), generation, projectId, "diff", resource) ||
      !matchesRequestIntent(get(), requestKey, requestGeneration)
    ) {
      return;
    }
    set((state) => ({
      diffs: boundedResourceStateUpdate(
        state.diffs,
        key,
        { phase: "error", message: errorMessage(error) },
        MAX_CACHED_DIFFS,
      ),
    }));
  }
}
