import { sameResource } from "../domain/resourceIdentity";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";

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
  const intents = { ...current };
  delete intents[key];
  intents[key] = token;
  const overflow = Object.keys(intents).length - MAX_REQUEST_GENERATIONS;
  if (overflow > 0) {
    for (const expired of Object.keys(intents).slice(0, overflow)) {
      delete intents[expired];
    }
  }
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
