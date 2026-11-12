import { sameResource } from "../domain/resourceIdentity";
import type {
  ProjectTransitionResult,
  ResourceRef,
  WorkbenchTab,
} from "../domain/workbench";
import { boundedRecordUpdate } from "./resourceStateHelpers";

const MAX_PROJECT_GENERATION = Number.MAX_SAFE_INTEGER;

interface ProjectIntentState {
  projectGeneration: number;
  project: { id: string; roots: Array<{ id: string }> } | null;
  tabs: WorkbenchTab[];
  activeTabId: string | null;
}

interface RequestIntentState {
  requestGenerations: Record<string, number>;
}

export const MAX_REQUEST_GENERATIONS = 128;

export function issueRequestIntent(
  current: Record<string, number>,
  key: string,
  sequence: number,
): {
  intents: Record<string, number>;
  token: number;
  sequence: number;
} {
  const token = nextProjectGeneration(sequence);
  const intents = boundedRecordUpdate(
    current,
    [key],
    token,
    MAX_REQUEST_GENERATIONS,
  );
  return { intents, token, sequence: token };
}

export function nextProjectGeneration(current: number): number {
  if (!Number.isSafeInteger(current) || current < 0) {
    throw new Error("project generation is corrupt");
  }
  if (current >= MAX_PROJECT_GENERATION) {
    throw new Error(
      "project generation limit reached; restart Sage to continue",
    );
  }
  return current + 1;
}

export function validateTransitionIntent(
  result: ProjectTransitionResult,
  expectedTransitionEpoch: number,
) {
  const responseEpoch = result.committed
    ? expectedTransitionEpoch + 1
    : expectedTransitionEpoch;
  if (
    !Number.isSafeInteger(expectedTransitionEpoch) ||
    !Number.isSafeInteger(result.transitionEpoch) ||
    result.transitionEpoch !== responseEpoch
  ) {
    throw new Error("native project transition returned a stale intent epoch");
  }
}

/** No project transition is running and the native epoch is still `epoch`. */
export function idleAtEpoch(
  state: { projectTransitioning: boolean; nativeTransitionEpoch: number },
  epoch: number,
): boolean {
  return !state.projectTransitioning && state.nativeTransitionEpoch === epoch;
}

export function matchesProjectIntent(
  state: ProjectIntentState,
  generation: number,
  projectId: string,
): boolean {
  return (
    state.projectGeneration === generation && state.project?.id === projectId
  );
}

export function matchesResourceIntent(
  state: ProjectIntentState,
  generation: number,
  projectId: string,
  resource: ResourceRef,
): boolean {
  const project = state.project;
  return (
    project !== null &&
    matchesProjectIntent(state, generation, projectId) &&
    project.roots.some((root) => root.id === resource.rootId)
  );
}

export function matchesActiveTabIntent(
  state: ProjectIntentState,
  generation: number,
  projectId: string,
  kind: "file" | "diff",
  resource: ResourceRef,
): boolean {
  if (!matchesResourceIntent(state, generation, projectId, resource)) {
    return false;
  }
  const active = state.tabs.find((tab) => tab.id === state.activeTabId);
  return active?.kind === kind && sameResource(active.resource, resource);
}

export function hasTabResourceIntent(
  state: ProjectIntentState,
  generation: number,
  projectId: string,
  kind: "file" | "diff",
  resource: ResourceRef,
): boolean {
  return (
    matchesResourceIntent(state, generation, projectId, resource) &&
    state.tabs.some(
      (tab) => tab.kind === kind && sameResource(tab.resource, resource),
    )
  );
}

export function matchesRequestIntent(
  state: RequestIntentState,
  key: string,
  generation: number,
): boolean {
  return state.requestGenerations[key] === generation;
}

interface RepositoryWorkState {
  projectTransitioning: boolean;
  repositoryRefreshing: boolean;
  repositoryMutation: unknown;
  repositoryCommit: unknown;
  repositoryCheckout: unknown;
  repositoryRemote: unknown;
}

/** True while a project switch or any repository operation is running. */
export function repositoryBusy(state: RepositoryWorkState): boolean {
  return Boolean(
    state.projectTransitioning ||
    state.repositoryRefreshing ||
    state.repositoryMutation ||
    state.repositoryCommit ||
    state.repositoryCheckout ||
    state.repositoryRemote,
  );
}
