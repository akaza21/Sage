import type { StateCreator } from "zustand";

import type {
  CodexAccount,
  CodexAuthResponse,
  CodexAuthScope,
  CuratedBindingId,
  ProjectGeneration,
  SageProjectId,
} from "../domain/acp";
import type { HarnessDefinition, ModelOption } from "../domain/workbench";
import { MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES } from "../domain/chatOwnership";
import { acpHarnessDesktop } from "../services/acpHarnessDesktop";
import { desktop } from "../services/desktop";
import {
  cachedHarnessModels,
  harnessModelIdentity,
  rememberHarnessModels,
} from "./harnessModelCache";
import { accountChange, authCurrent, authScope } from "./harnessAuth";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import type { WorkbenchState } from "./store";
import { errorMessage } from "../domain/errors";

export type HarnessDiscoveryPhase =
  | "idle"
  | "checking"
  | "signInRequired"
  | "consent"
  | "waiting"
  | "ready"
  | "error";

export function preferredModelId(
  models: ModelOption[],
  previousModelId: string | null,
): string | null {
  return models.length === 0
    ? null
    : previousModelId && models.some((model) => model.id === previousModelId)
      ? previousModelId
      : (models.find((model) => model.isDefault)?.id ?? models[0]?.id ?? null);
}

export interface HarnessSlice {
  harnesses: HarnessDefinition[];
  selectedHarnessId: string | null;
  models: ModelOption[];
  selectedModelId: string | null;
  harnessRequestGeneration: number;
  harnessDiscoveryPhase: HarnessDiscoveryPhase;
  harnessDiscoveryError: string | null;
  /** The account the selected harness reported at its last discovery. */
  harnessAccount: CodexAccount | null;
  selectHarness: (harnessId: string) => Promise<void>;
  applyHarnessDiscovery: (harnesses: HarnessDefinition[]) => Promise<void>;
  loadHarnessModelsForProjection: (
    harnessId: string,
    selectedModelId: string | null,
    projectGeneration: number,
    projectId: string,
  ) => Promise<void>;
  retryHarnessDiscovery: () => Promise<void>;
  requestHarnessSignIn: () => void;
  confirmHarnessSignIn: () => Promise<void>;
  cancelHarnessSignIn: () => Promise<void>;
  logoutHarness: () => Promise<void>;
  signInHarnessWith: (
    call: (scope: CodexAuthScope) => Promise<CodexAuthResponse>,
  ) => Promise<void>;
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

  const revalidate = (
    harnessId: string,
    identity: string | null,
    generation: number,
    projectId: string,
    requestGeneration: number,
  ) => {
    desktop
      .listHarnessModels(harnessId)
      .then((models) => {
        rememberHarnessModels(identity, models);
        if (!current(generation, projectId, requestGeneration, harnessId)) {
          return;
        }
        const selected = get().selectedModelId;
        set({
          models,
          selectedModelId: models.some((model) => model.id === selected)
            ? selected
            : null,
          harnessDiscoveryPhase: "ready",
          harnessDiscoveryError: null,
        });
      })
      .catch(() => undefined);
  };

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
    const detecting = harness?.installState === "detecting";
    if (harness && !detecting && harness.installState !== "available") {
      set({
        selectedHarnessId: harnessId,
        selectedModelId: previousModelId,
        models: [],
        harnessRequestGeneration: requestGeneration,
        harnessDiscoveryPhase: "ready",
        harnessDiscoveryError: null,
        harnessAccount: null,
      });
      return;
    }
    const identity = usesAccountModelDiscovery
      ? null
      : harnessModelIdentity(harness);
    const cached = cachedHarnessModels(identity);
    set({
      selectedHarnessId: harnessId,
      models: cached ?? [],
      selectedModelId: cached
        ? preferredModelId(cached, previousModelId)
        : null,
      harnessRequestGeneration: requestGeneration,
      harnessDiscoveryPhase: cached
        ? "ready"
        : detecting || usesAccountModelDiscovery
          ? "checking"
          : "idle",
      harnessDiscoveryError: null,
      harnessAccount: null,
    });
    if (detecting) return;
    if (cached) {
      revalidate(harnessId, identity, generation, projectId, requestGeneration);
      return;
    }
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
        const response = await acpHarnessDesktop.discoverCodex({
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
        set({ harnessAccount: response.account });
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
      rememberHarnessModels(identity, models);
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
      const message = errorMessage(error);
      set({ harnessDiscoveryPhase: "error", harnessDiscoveryError: message });
    }
  };

  const changeAccount = accountChange(set, get);

  return {
    harnesses: [],
    selectedHarnessId: null,
    models: [],
    selectedModelId: null,
    harnessRequestGeneration: 0,
    harnessDiscoveryPhase: "idle",
    harnessDiscoveryError: null,
    harnessAccount: null,
    selectHarness: async (harnessId) => {
      if (get().projectTransitioning) return;
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (!projectId) return;
      const previous =
        get().selectedHarnessId === harnessId ? get().selectedModelId : null;
      await load(harnessId, previous, generation, projectId);
    },
    applyHarnessDiscovery: async (harnesses) => {
      const previous = get().harnesses.find(
        (candidate) => candidate.id === get().selectedHarnessId,
      );
      // Discovery describes installed harnesses. Keep restored bindings visible
      // until the next Project projection replaces this bounded catalog.
      harnesses = [
        ...harnesses,
        ...get()
          .harnesses.filter(
            (previous) => !harnesses.some((next) => next.id === previous.id),
          )
          .slice(0, MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES)
          .map((previous): HarnessDefinition => ({
            ...previous,
            installState: "unavailable",
            accountModelDiscovery: false,
          })),
      ];
      set({ harnesses });
      const projectId = get().project?.id;
      if (!projectId || get().projectTransitioning) return;
      const generation = get().projectGeneration;
      const selectable = harnesses.filter(
        (candidate) =>
          candidate.transport === "acp" &&
          candidate.installState === "available",
      );
      const selected = get().selectedHarnessId;
      const next = selected
        ? harnesses.find((candidate) => candidate.id === selected)
        : selectable[0];
      if (!next) return;
      const retained = next.id === selected;
      if (
        retained &&
        get().harnessDiscoveryPhase === "ready" &&
        harnessModelIdentity(previous) === harnessModelIdentity(next) &&
        previous?.accountModelDiscovery === next.accountModelDiscovery
      )
        return;
      await load(
        next.id,
        retained ? get().selectedModelId : null,
        generation,
        projectId,
      );
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
    requestHarnessSignIn: () => {
      if (get().harnessDiscoveryPhase === "signInRequired") {
        set({ harnessDiscoveryPhase: "consent", harnessDiscoveryError: null });
      }
    },
    confirmHarnessSignIn: async () => {
      const scope = authScope(get());
      if (!scope || get().harnessDiscoveryPhase !== "consent") return;
      const requestGeneration = get().harnessRequestGeneration;
      set({ harnessDiscoveryPhase: "waiting", harnessDiscoveryError: null });
      try {
        const response = await acpHarnessDesktop.startCodexAuth(scope);
        if (!authCurrent(get(), scope, requestGeneration)) return;
        if (response.phase !== "signedIn") {
          set({ harnessDiscoveryPhase: "signInRequired" });
          return;
        }
        await get().retryHarnessDiscovery();
      } catch (error) {
        if (!authCurrent(get(), scope, requestGeneration)) return;
        set({
          harnessDiscoveryPhase: "error",
          harnessDiscoveryError: errorMessage(error),
        });
      }
    },
    cancelHarnessSignIn: async () => {
      if (get().harnessDiscoveryPhase === "consent") {
        set({ harnessDiscoveryPhase: "signInRequired" });
        return;
      }
      const scope = authScope(get());
      if (!scope || get().harnessDiscoveryPhase !== "waiting") return;
      await acpHarnessDesktop.cancelCodexAuth(scope);
    },
    logoutHarness: async () => {
      if (get().harnessDiscoveryPhase === "ready")
        await changeAccount(acpHarnessDesktop.logoutCodexAuth, "signedOut");
    },
    signInHarnessWith: async (call) => {
      const phase = get().harnessDiscoveryPhase;
      if (phase !== "checking" && phase !== "waiting")
        await changeAccount(call, "signedIn");
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
