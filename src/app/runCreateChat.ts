import type { StoreApi } from "zustand";

import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
} from "../domain/workbench";
import type { ChatPort } from "../ports/workbenchPorts";
import {
  commitIdentityChatResult,
  errorMessage,
  type ProjectionOutcome,
} from "./chatTransitions";
import {
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import { currentProjectLayout } from "./projectProjection";
import type { WorkbenchState } from "./store";

export async function runCreateChat(
  input: {
    set: StoreApi<WorkbenchState>["setState"];
    get: StoreApi<WorkbenchState>["getState"];
    applyProjection: (
      snapshot: BootstrapSnapshot,
      generation: number,
    ) => Promise<void>;
    loadAuthoritativeProjection: (
      generation: number,
      classify: (snapshot: BootstrapSnapshot) => ProjectionOutcome | null,
    ) => Promise<ProjectionOutcome>;
    chat: ChatPort;
  },
  projectId?: string,
): Promise<void> {
  const { set, get, applyProjection, loadAuthoritativeProjection, chat } =
    input;
  if (get().projectTransitioning) return;
  const initial = currentProjectLayout(get());
  const sourceProjectId = projectId ?? initial?.projectId;
  const foreign =
    typeof sourceProjectId === "string" &&
    sourceProjectId !== initial?.projectId;
  if (!initial) {
    if (foreign) return;
    await get().ensureLocalProject();
    return;
  }
  if (!sourceProjectId) return;
  set({ projectTransitioning: true, error: null });
  const expectedTransitionEpoch = get().nativeTransitionEpoch;
  try {
    await get().flushActiveEditor();
  } catch (error) {
    set({
      projectTransitioning: false,
      error: `Chat creation blocked by editor snapshot failure: ${errorMessage(error)}`,
    });
    return;
  }
  const current = currentProjectLayout(get());
  if (!current) {
    set({
      projectTransitioning: false,
      error: "active Project disappeared before Chat creation",
    });
    return;
  }
  if (foreign) {
    await commitIdentityChatResult({
      set,
      get,
      applyProjection,
      loadAuthoritativeProjection,
      failedMessage: "native Chat creation did not commit",
      invoke: () =>
        chat.createChat(current, expectedTransitionEpoch, sourceProjectId),
    });
    return;
  }
  await createOnActiveProject({
    set,
    get,
    applyProjection,
    loadAuthoritativeProjection,
    chat,
    current,
    expectedTransitionEpoch,
  });
}

async function createOnActiveProject(input: {
  set: StoreApi<WorkbenchState>["setState"];
  get: StoreApi<WorkbenchState>["getState"];
  applyProjection: (
    snapshot: BootstrapSnapshot,
    generation: number,
  ) => Promise<void>;
  loadAuthoritativeProjection: (
    generation: number,
    classify: (snapshot: BootstrapSnapshot) => ProjectionOutcome | null,
  ) => Promise<ProjectionOutcome>;
  chat: ChatPort;
  current: ProjectLayoutRequest;
  expectedTransitionEpoch: number;
}): Promise<void> {
  const { set, get, applyProjection, loadAuthoritativeProjection, chat } =
    input;
  const priorChatIds = new Set(
    input.current.tabs
      .filter((tab) => tab.kind === "chat")
      .map((tab) => tab.id),
  );
  const classify = (snapshot: BootstrapSnapshot) => {
    if (snapshot.project?.id !== input.current.projectId) return null;
    const active = snapshot.tabs.find(
      (tab) => tab.id === snapshot.activeTabId && tab.kind === "chat",
    );
    if (
      snapshot.tabs.length === input.current.tabs.length + 1 &&
      active &&
      !priorChatIds.has(active.id)
    ) {
      return "committed" as const;
    }
    if (
      snapshot.tabs.length === input.current.tabs.length &&
      snapshot.activeTabId === input.current.activeTabId
    ) {
      return "unchanged" as const;
    }
    return null;
  };
  let committed = false;
  let mutationInvoked = false;
  try {
    const latest = currentProjectLayout(get());
    if (!latest)
      throw new Error("active Project disappeared before Chat creation");
    mutationInvoked = true;
    const result = await chat.createChat(latest, input.expectedTransitionEpoch);
    validateTransitionIntent(result, input.expectedTransitionEpoch);
    if (!result.committed) {
      set({
        projectTransitioning: false,
        error: result.recoveryError ?? "native Chat creation did not commit",
      });
      return;
    }
    committed = true;
    const generation = nextProjectGeneration(get().projectGeneration);
    set({ projectGeneration: generation, repositoryRefreshing: false });
    if (result.snapshot) {
      if (result.snapshot.transitionEpoch !== result.transitionEpoch) {
        throw new Error(
          "native Chat creation returned inconsistent intent metadata",
        );
      }
      if (classify(result.snapshot) !== "committed") {
        throw new Error("native Chat creation returned an invalid projection");
      }
      await applyProjection(result.snapshot, generation);
    } else {
      await loadAuthoritativeProjection(generation, classify);
    }
  } catch (error) {
    if (committed) {
      set({
        phase: "error",
        projectTransitioning: false,
        repositoryRefreshing: false,
        error: `Chat creation committed but projection recovery failed: ${errorMessage(error)}`,
      });
    } else if (mutationInvoked) {
      const generation = nextProjectGeneration(get().projectGeneration);
      set({ projectGeneration: generation, repositoryRefreshing: false });
      try {
        const outcome = await loadAuthoritativeProjection(generation, classify);
        if (outcome === "unchanged") set({ error: errorMessage(error) });
      } catch (recoveryError) {
        set({
          phase: "error",
          projectTransitioning: false,
          error: `Chat creation state is unknown: ${errorMessage(recoveryError)}`,
        });
      }
    } else {
      set({ projectTransitioning: false, error: errorMessage(error) });
    }
  }
}
