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
import { accountChange } from "./harnessAuth";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import type { WorkbenchState } from "./store";
import { safeFailure } from "../services/acpDesktopCommand";
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
  models: ModelOption[] | null,
  previousModelId: string | null,
): string | null {
  return !models?.length
    ? null
    : previousModelId && models.some((model) => model.id === previousModelId)
      ? previousModelId
      : (models.find((model) => model.isDefault)?.id ?? models[0].id);
}

export interface HarnessSlice {
  harnesses: HarnessDefinition[];
  selectedHarnessId: string | null;
  models: ModelOption[];
  selectedModelId: string | null;
  harnessRequestGeneration: number;
  harnessDiscoveryPhase: HarnessDiscoveryPhase;
  harnessDiscoveryError: string | null;
  harnessAccount: CodexAccount | null;
  // Native epoch ms is comparable with persisted turn terminal times.
  harnessAccountReadStartedAt: number | null;
  pendingHarnessSignIn: CodexAuthScope | null;
  selectHarness: (harnessId: string, refresh?: boolean) => Promise<void>;
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
    pending?: "checking" | "waiting",
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
    refresh?: boolean,
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
      harnessAccountReadStartedAt: null,
      ...(!refresh && {
        models: cached ?? [],
        selectedModelId: preferredModelId(cached, previousModelId),
      }),
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
        if (!harness.bindingId) {
          throw new Error(
            "This account-model binding is not installed and admitted yet.",
          );
        }
        const nativeGeneration = String(
          get().nativeTransitionEpoch,
        ) as ProjectGeneration;
        const response = await acpHarnessDesktop.discoverCodex({
          refresh,
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
        // The desktop decoder already owns a fresh, validated catalog.
        const models = response.models.slice();
        // Signed out, the catalog is still listed; sign-in gates a send.
        set({
          harnessAccount: response.account,
          harnessAccountReadStartedAt: response.accountReadStartedAt,
          models,
          selectedModelId: preferredModelId(models, previousModelId),
          harnessDiscoveryPhase:
            response.account === "signedOut" ? "signInRequired" : "ready",
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
      set({
        harnessDiscoveryPhase: "error",
        harnessDiscoveryError: message,
        harnessAccountReadStartedAt:
          safeFailure(error).accountReadStartedAt ?? null,
      });
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
    harnessAccountReadStartedAt: null,
    pendingHarnessSignIn: null,
    selectHarness: async (harnessId, refresh) => {
      if (get().projectTransitioning) return;
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (!projectId) return;
      const previous =
        get().selectedHarnessId === harnessId ? get().selectedModelId : null;
      await load(harnessId, previous, generation, projectId, refresh);
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
      if (harnessId) await get().selectHarness(harnessId, true);
    },
    requestHarnessSignIn: () => {
      if (get().harnessDiscoveryPhase === "signInRequired") {
        set({ harnessDiscoveryPhase: "consent", harnessDiscoveryError: null });
      }
    },
    confirmHarnessSignIn: async () => {
      if (get().harnessDiscoveryPhase === "consent")
        await changeAccount(
          acpHarnessDesktop.startCodexAuth,
          "signedIn",
          "waiting",
        );
    },
    cancelHarnessSignIn: async () => {
      const pending = get().pendingHarnessSignIn;
      if (pending) {
        await acpHarnessDesktop.cancelCodexAuth(pending);
        return;
      }
      if (get().harnessDiscoveryPhase === "consent") {
        set({ harnessDiscoveryPhase: "signInRequired" });
      }
    },
    logoutHarness: async () => {
      if (get().harnessDiscoveryPhase === "ready")
        await changeAccount(
          acpHarnessDesktop.logoutCodexAuth,
          "signedOut",
          "checking",
        );
    },
    signInHarnessWith: async (call, pending = "checking") => {
      const phase = get().harnessDiscoveryPhase;
      if (phase !== "checking" && phase !== "waiting")
        await changeAccount(call, "signedIn", pending);
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
