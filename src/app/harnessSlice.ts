import type { StateCreator } from "zustand";

import type {
  CuratedBindingId,
  ProjectGeneration,
  SageProjectId,
} from "../domain/acp";
import type { HarnessDefinition, ModelOption } from "../domain/workbench";
import { preferredModelId } from "../features/harnesses/modelSelection";
import { acpDesktop } from "../services/acpDesktop";
import { desktop } from "../services/desktop";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import type { WorkbenchState } from "./store";

export type HarnessDiscoveryPhase =
  "idle" | "checking" | "signInRequired" | "ready" | "error";

export interface HarnessSlice {
  harnesses: HarnessDefinition[];
  selectedHarnessId: string | null;
  models: ModelOption[];
  selectedModelId: string | null;
  harnessRequestGeneration: number;
  harnessDiscoveryPhase: HarnessDiscoveryPhase;
  harnessDiscoveryError: string | null;
  selectHarness: (harnessId: string) => Promise<void>;
  loadHarnessModelsForProjection: (
    harnessId: string,
    selectedModelId: string | null,
    projectGeneration: number,
    projectId: string,
  ) => Promise<void>;
  retryHarnessDiscovery: () => Promise<void>;
  selectModel: (modelId: string) => void;
}

export const createHarnessSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  HarnessSlice
> = (set, get) => {
  const current = (
    generation: number,
    projectId: string,
    requestGeneration: number,
    harnessId: string,
  ) =>
    matchesProjectIntent(get(), generation, projectId) &&
    get().harnessRequestGeneration === requestGeneration &&
    get().selectedHarnessId === harnessId;

  const load = async (
    harnessId: string,
    previousModelId: string | null,
    generation: number,
    projectId: string,
  ) => {
    const requestGeneration = nextProjectGeneration(
      get().harnessRequestGeneration,
    );
    const harness = get().harnesses.find(
      (candidate) => candidate.id === harnessId,
    );
    const usesAccountModelDiscovery =
      harness?.transport === "acp" && harness.accountModelDiscovery;
    set({
      selectedHarnessId: harnessId,
      models: [],
      selectedModelId: null,
      harnessRequestGeneration: requestGeneration,
      harnessDiscoveryPhase: usesAccountModelDiscovery ? "checking" : "idle",
      harnessDiscoveryError: null,
    });
    try {
      if (usesAccountModelDiscovery) {
        if (harness.installState !== "available" || !harness.bindingId) {
          throw new Error(
            "This account-model binding is not installed and admitted yet.",
          );
        }
        const nativeGeneration = String(
          get().nativeTransitionEpoch,
        ) as ProjectGeneration;
        const response = await acpDesktop.discoverCodex({
          projectId: projectId as SageProjectId,
          projectGeneration: nativeGeneration,
          bindingId: harness.bindingId as CuratedBindingId,
        });
        const currentRoots = get().project?.roots;
        const currentRootId =
          currentRoots?.length === 1 ? currentRoots[0].id : null;
        if (
          !current(generation, projectId, requestGeneration, harnessId) ||
          String(get().nativeTransitionEpoch) !== nativeGeneration ||
          response.rootId !== currentRootId ||
          response.projectId !== projectId ||
          response.projectGeneration !== nativeGeneration ||
          response.bindingId !== harness.bindingId
        ) {
          return;
        }
        if (response.account === "signedOut") {
          set({ harnessDiscoveryPhase: "signInRequired", error: null });
          return;
        }
        const models = response.models.map((model) => ({
          id: model.id,
          name: model.name,
          isDefault: model.isDefault,
        }));
        set({
          models,
          selectedModelId: preferredModelId(models, previousModelId),
          harnessDiscoveryPhase: "ready",
          harnessDiscoveryError: null,
          error: null,
        });
        return;
      }

      const models = await desktop.listHarnessModels(harnessId);
      if (current(generation, projectId, requestGeneration, harnessId)) {
        set({
          models,
          selectedModelId: preferredModelId(models, previousModelId),
          harnessDiscoveryPhase: "ready",
          harnessDiscoveryError: null,
          error: null,
        });
      }
    } catch (error) {
      if (!current(generation, projectId, requestGeneration, harnessId)) return;
      const message = error instanceof Error ? error.message : String(error);
      if (usesAccountModelDiscovery) {
        set({ harnessDiscoveryPhase: "error", harnessDiscoveryError: message });
      } else {
        set({ error: message, harnessDiscoveryPhase: "error" });
      }
    }
  };

  return {
    harnesses: [],
    selectedHarnessId: null,
    models: [],
    selectedModelId: null,
    harnessRequestGeneration: 0,
    harnessDiscoveryPhase: "idle",
    harnessDiscoveryError: null,
    selectHarness: async (harnessId) => {
      if (get().projectTransitioning) return;
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (!projectId) return;
      const previous =
        get().selectedHarnessId === harnessId ? get().selectedModelId : null;
      await load(harnessId, previous, generation, projectId);
    },
    loadHarnessModelsForProjection: async (
      harnessId,
      selectedModelId,
      projectGeneration,
      projectId,
    ) => {
      if (!matchesProjectIntent(get(), projectGeneration, projectId)) return;
      await load(harnessId, selectedModelId, projectGeneration, projectId);
    },
    retryHarnessDiscovery: async () => {
      const harnessId = get().selectedHarnessId;
      if (harnessId) await get().selectHarness(harnessId);
    },
    selectModel: (selectedModelId) => {
      if (
        get().harnessDiscoveryPhase === "ready" &&
        get().models.some((model) => model.id === selectedModelId)
      ) {
        set({ selectedModelId });
      }
    },
  };
};
