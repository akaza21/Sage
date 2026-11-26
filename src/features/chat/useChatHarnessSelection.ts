import {
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { useStore } from "zustand";

import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import { preferredModelId } from "../../app/harnessSlice";
import {
  cachedHarnessModels,
  harnessModelIdentity,
  harnessModelCache,
} from "../../app/harnessModelCache";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import type { HarnessDefinition } from "../../domain/harness";
import type { SearchablePickerOption } from "../../ui/SearchablePicker";
import { useAcpChatProjection } from "./acp";
import { harnessAvailability } from "./harnessAvailability";
import { turnBindingId } from "./acp/acpTurnState";
import {
  harnessSignInEvidence,
  signInEvidenceListeners,
} from "../../services/acpHarnessDesktop";
import {
  useInheritedComposerModel,
  useInheritedStore,
} from "./inheritedComposerModelState";

export function harnessPickerCandidates(
  harnesses: readonly HarnessDefinition[],
  selectedId?: string | null,
  selectedSubtitle?: string,
): SearchablePickerOption[] {
  return harnesses
    .map<SearchablePickerOption>((harness) => {
      const availability = harnessAvailability(harness);
      const disabledReason =
        harness.id === selectedId && availability.disabledReason
          ? "Unavailable"
          : availability.disabledReason;
      const status = harness.versionChecking
        ? "version checking…"
        : harness.source === "development"
          ? "test only"
          : availability.label;
      return {
        id: harness.id,
        icon: harness.brand ?? "generic",
        label: harness.name,
        subtitle: harness.id === selectedId ? selectedSubtitle : undefined,
        accessibleLabel: `${harness.name} — ${status}${disabledReason ? `. ${disabledReason}` : ""}`,
        disabledReason,
      };
    })
    .sort(
      (left, right) =>
        Number(!!left.disabledReason) - Number(!!right.disabledReason),
    );
}

/**
 * The composer's one open popover. `disabled` closes them all; `busy` (a
 * running turn) closes all but the "+" menu, whose attachments a queued
 * follow-up still needs.
 */
export function useComposerPopovers(disabled = false, busy = false) {
  type Kind = "options" | "access" | "harness" | "model" | "effort" | "project";
  const [selected, setActive] = useState<Kind | null>(null);
  const blocked = (kind: Kind) => disabled || (busy && kind !== "options");
  const active = selected && blocked(selected) ? null : selected;
  const optionsId = useId();
  const modelId = useId();
  const optionsTrigger = useRef<HTMLButtonElement>(null);
  const modelTrigger = useRef<HTMLButtonElement>(null);

  if (selected !== active) setActive(active);

  return {
    active,
    modelId,
    modelTrigger,
    optionsId,
    optionsTrigger,
    dismiss: () => setActive(null),
    setHarnessOpen: (open: boolean) => {
      if (!blocked("harness"))
        setActive((current) =>
          open ? "harness" : current === "harness" ? null : current,
        );
    },
    toggle: (kind: Kind) => {
      if (!blocked(kind))
        setActive((current) => (current === kind ? null : kind));
    },
  };
}

export function useChatHarnessSelection(chatId?: string): {
  page: AcpChatPage | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  discoveryPhase: HarnessDiscoveryPhase;
  chooseHarness: (harnessId: string) => Promise<void>;
  chooseModel: (modelId: string) => void;
} {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const globallySelectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const selectedModelId = useWorkbenchStore((state) =>
    chatId === undefined
      ? state.selectedModelId
      : (state.chatUiById[chatId]?.modelId ?? null),
  );
  const inherited = useInheritedComposerModel(chatId ?? null);
  const ownedHarnessId = useWorkbenchStore((state) =>
    chatId ? state.chatUiById[chatId]?.harnessId : null,
  );
  const harnessDiscoveryPhase = useWorkbenchStore(
    (state) => state.harnessDiscoveryPhase,
  );
  const {
    captureChatOwnership,
    setChatHarnessSelection,
    selectHarness,
    selectModel,
  } = useWorkbenchStore.getState();
  const selectedHarnessId = ownedHarnessId ?? globallySelectedHarnessId;
  const globalModels = useWorkbenchStore((state) =>
    selectedHarnessId === state.selectedHarnessId ? state.models : null,
  );
  const harnessDefinition =
    harnesses.find((harness) => harness.id === selectedHarnessId) ?? null;
  const page = useAcpChatProjection(
    chatId !== undefined || harnessDefinition?.transport === "acp",
    chatId,
  );
  const modelIdentity = harnessModelIdentity(harnessDefinition);
  const cachedModels = useStore(harnessModelCache, () =>
    cachedHarnessModels(modelIdentity),
  );
  const models = globalModels ?? cachedModels;
  const selectedSession =
    page?.sessions.find(
      (candidate) =>
        candidate.id === page.currentSessionId &&
        !candidate.legacy &&
        !["closed", "failed", "incompatible"].includes(candidate.state),
    ) ??
    (!page?.currentSessionId
      ? page?.sessions.filter((candidate) => !candidate.legacy).at(-1)
      : null);
  const sessionHarnessId = selectedSession
    ? (harnesses.find(
        (harness) =>
          harness.transport === "acp" &&
          harness.bindingId === selectedSession.bindingId,
      )?.id ?? null)
    : null;
  const sessionModelId = selectedSession?.modelId ?? null;
  // After a sign-in failure, sends wait for newer signed-in evidence or a
  // newer turn (Retry), instead of being refused again.
  const latest = page?.turns.at(-1);
  const refused =
    latest?.state === "failed" &&
    latest.terminalReason?.kind === "authRequired" &&
    latest.sessionId === page?.currentSessionId
      ? latest
      : null;
  const evidence = useSyncExternalStore(subscribeSignInEvidence, () =>
    refused
      ? harnessSignInEvidence.get(turnBindingId(page!, refused) ?? "")
      : undefined,
  );
  const discoveryPhase: HarnessDiscoveryPhase =
    refused &&
    !(evidence?.[1] === "signedIn" && evidence[0] > (refused.terminalAt ?? 0))
      ? "signInRequired"
      : sessionHarnessId &&
    sessionHarnessId === selectedHarnessId &&
    sessionModelId === selectedModelId
      ? "ready"
      : chatId !== undefined && globalModels === null
        ? "ready"
        : harnessDiscoveryPhase;
  useLayoutEffect(() => {
    // Materialize pre-session defaults through the same owned chat selection
    // as a draft's explicit pick, so rendering and native start agree.
    if (
      !chatId ||
      !page ||
      (ownedHarnessId &&
        (selectedModelId !== null || sessionHarnessId === ownedHarnessId))
    )
      return;
    const useSession = !ownedHarnessId && sessionHarnessId;
    if (!useSession && discoveryPhase !== "ready") return;
    const harnessId = useSession || selectedHarnessId;
    const modelId = useSession
      ? sessionModelId
      : (inherited?.id ?? preferredModelId(models ?? [], null));
    if (!harnessId || (ownedHarnessId && modelId === null)) return;
    const ownership = captureChatOwnership(chatId);
    if (ownership) setChatHarnessSelection(ownership, { harnessId, modelId });
  }, [
    captureChatOwnership,
    chatId,
    ownedHarnessId,
    discoveryPhase,
    inherited?.id,
    models,
    page,
    selectedHarnessId,
    selectedModelId,
    sessionHarnessId,
    sessionModelId,
    setChatHarnessSelection,
  ]);
  const selectionRequest = useRef(0);
  const chooseHarness = useCallback(
    async (harnessId: string) => {
      const request = ++selectionRequest.current;
      const ownership = captureChatOwnership(chatId);
      if (ownership) {
        if (harnessId !== selectedHarnessId) {
          useInheritedStore.setState(
            ({ [ownership.chatId]: _inherited, ...remaining }) => remaining,
            true,
          );
        }
        setChatHarnessSelection(ownership, { harnessId, modelId: null });
      }
      await selectHarness(harnessId);
      const state = useWorkbenchStore.getState();
      if (
        !ownership ||
        request !== selectionRequest.current ||
        state.selectedHarnessId !== harnessId
      )
        return;
      setChatHarnessSelection(ownership, {
        harnessId,
        modelId: state.selectedModelId,
      });
    },
    [
      captureChatOwnership,
      chatId,
      selectHarness,
      selectedHarnessId,
      setChatHarnessSelection,
    ],
  );
  const chooseModel = useCallback(
    (modelId: string) => {
      const ownership = captureChatOwnership(chatId);
      selectModel(modelId);
      if (ownership)
        setChatHarnessSelection(ownership, {
          harnessId: selectedHarnessId,
          modelId,
        });
    },
    [
      captureChatOwnership,
      chatId,
      selectModel,
      selectedHarnessId,
      setChatHarnessSelection,
    ],
  );
  return {
    page,
    selectedHarnessId,
    selectedModelId,
    discoveryPhase,
    chooseHarness,
    chooseModel,
  };
}

function subscribeSignInEvidence(listener: () => void) {
  signInEvidenceListeners.add(listener);
  return () => {
    signInEvidenceListeners.delete(listener);
  };
}
