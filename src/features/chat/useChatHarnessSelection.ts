import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type { HarnessDefinition, ModelOption } from "../../domain/harness";
import {
  cachedHarnessModels,
  harnessModelIdentity,
} from "../../app/harnessModelCache";
import type { SearchablePickerOption } from "../../ui/SearchablePicker";
import { harnessIconKind } from "./composerChrome";
import { harnessAvailability } from "./harnessAvailability";
import type { AcpChatPage } from "../../domain/acp";
import { useAcpChatProjection } from "./acp";

export function harnessPickerCandidates(
  harnesses: readonly HarnessDefinition[],
  selectedHarnessId: string | null,
  selectedModelId: string | null,
  models: readonly ModelOption[],
  modelLabel: string,
  effortLabel: string | null,
): SearchablePickerOption[] {
  return harnesses.map((harness) => {
    const icon = harnessIconKind(harness.name);
    const availability = harnessAvailability(harness);
    const name = `${harness.name}${harness.version ? ` — ${harness.version}` : ""}`;
    const catalog =
      harness.id === selectedHarnessId
        ? models
        : (cachedHarnessModels(harnessModelIdentity(harness)) ?? []);
    const current =
      catalog.find(
        (model) =>
          model.id ===
          (harness.id === selectedHarnessId ? selectedModelId : null),
      ) ??
      catalog.find((model) => model.isDefault) ??
      catalog[0];
    const detail =
      harness.id === selectedHarnessId && modelLabel !== "Model"
        ? `${modelLabel}${effortLabel ? ` ${effortLabel}` : ""}`
        : (current?.name ?? "Default model");
    const status = harness.versionChecking
      ? "version checking…"
      : harness.source === "development"
        ? "test only"
        : availability.label;
    const statusLine = ` — ${status}`;
    const showStatus =
      harness.installState !== "available" ||
      harness.versionChecking ||
      harness.characterisation === "uncharacterised" ||
      harness.source === "development";
    return {
      id: harness.id,
      icon,
      logo: `/harness-${icon ?? "generic"}.svg`,
      label: name,
      triggerLabel: harness.name,
      secondaryLabel: showStatus ? statusLine : detail,
      accessibleLabel: `${name}${statusLine}${availability.disabledReason ?? ""}`,
      disabledReason: availability.disabledReason,
    };
  });
}

export function useComposerPopovers(disabled = false) {
  const [selected, setActive] = useState<"options" | "model" | null>(null);
  const active = disabled ? null : selected;
  const optionsId = useId();
  const modelId = useId();
  const optionsTrigger = useRef<HTMLButtonElement>(null);
  const modelTrigger = useRef<HTMLButtonElement>(null);
  const optionsPanel = useRef<HTMLDivElement>(null);
  const modelPanel = useRef<HTMLDivElement>(null);
  const trigger = active === "options" ? optionsTrigger : modelTrigger;
  const panel = active === "options" ? optionsPanel : modelPanel;

  useEffect(() => {
    if (disabled) queueMicrotask(() => setActive(null));
  }, [disabled]);

  useLayoutEffect(() => {
    if (!active) return;
    (
      panel.current?.querySelector<HTMLButtonElement>(
        "button:not(:disabled)",
      ) ?? panel.current
    )?.focus();
  }, [active, panel]);

  useEffect(() => {
    if (!active) return;
    const dismiss = () => {
      setActive(null);
      trigger.current?.focus();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      dismiss();
    };
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      if (panel.current?.contains(target) || trigger.current?.contains(target))
        return;
      if (target.closest('[role="listbox"], [role="combobox"]')) return;
      dismiss();
    };
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [active, panel, trigger]);

  return {
    active,
    modelId,
    modelPanel,
    modelTrigger,
    optionsId,
    optionsPanel,
    optionsTrigger,
    toggle: (kind: "options" | "model") => {
      if (!disabled) setActive((current) => (current === kind ? null : kind));
    },
  };
}

export function useChatHarnessSelection(chatId?: string): {
  page: AcpChatPage | null;
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  harnessDefinition: HarnessDefinition | null;
  discoveryPhase: HarnessDiscoveryPhase;
  chooseHarness: (harnessId: string) => Promise<void>;
  chooseModel: (modelId: string) => void;
} {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const globallySelectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const globallySelectedModelId = useWorkbenchStore(
    (state) => state.selectedModelId,
  );
  const chatSelection = useWorkbenchStore((state) =>
    chatId ? (state.chatUiById[chatId] ?? null) : null,
  );
  const harnessDiscoveryPhase = useWorkbenchStore(
    (state) => state.harnessDiscoveryPhase,
  );
  const captureChatOwnership = useWorkbenchStore(
    (state) => state.captureChatOwnership,
  );
  const setChatHarnessSelection = useWorkbenchStore(
    (state) => state.setChatHarnessSelection,
  );
  const selectHarness = useWorkbenchStore((state) => state.selectHarness);
  const selectModel = useWorkbenchStore((state) => state.selectModel);
  const selectedHarnessId =
    chatId === undefined
      ? globallySelectedHarnessId
      : (chatSelection?.harnessId ?? globallySelectedHarnessId);
  const selectedModelId =
    chatId === undefined
      ? globallySelectedModelId
      : (chatSelection?.modelId ?? null);
  const harnessDefinition =
    harnesses.find((harness) => harness.id === selectedHarnessId) ?? null;
  const page = useAcpChatProjection(
    chatId !== undefined || harnessDefinition?.transport === "acp",
    chatId,
  );
  const session = page?.sessions.find(
    (candidate) =>
      candidate.id === page.currentSessionId &&
      !candidate.legacy &&
      candidate.runtimeEpoch &&
      !["closed", "failed", "incompatible"].includes(candidate.state),
  );
  const historicalSession =
    !session && !page?.currentSessionId
      ? page?.sessions.filter((candidate) => !candidate.legacy).at(-1)
      : null;
  const selectedSession = session ?? historicalSession;
  const sessionHarnessId = selectedSession
    ? (harnesses.find(
        (harness) =>
          harness.transport === "acp" &&
          harness.bindingId === selectedSession.bindingId,
      )?.id ?? null)
    : null;
  const sessionSelection = useMemo(
    () =>
      sessionHarnessId
        ? {
            harnessId: sessionHarnessId,
            modelId: selectedSession?.modelId ?? null,
          }
        : null,
    [sessionHarnessId, selectedSession?.modelId],
  );
  const discoveryPhase =
    sessionSelection?.harnessId === selectedHarnessId &&
    sessionSelection.modelId === selectedModelId
      ? "ready"
      : chatId !== undefined && selectedHarnessId !== globallySelectedHarnessId
        ? "ready"
        : harnessDiscoveryPhase;
  useLayoutEffect(() => {
    if (!chatId || !page || chatSelection?.harnessId) return;
    const selection =
      sessionSelection ??
      (harnessDiscoveryPhase === "ready" && globallySelectedHarnessId
        ? {
            harnessId: globallySelectedHarnessId,
            modelId: globallySelectedModelId,
          }
        : null);
    if (!selection) return;
    const ownership = captureChatOwnership(chatId);
    if (!ownership) return;
    setChatHarnessSelection(ownership, selection);
  }, [
    captureChatOwnership,
    chatId,
    chatSelection?.harnessId,
    globallySelectedHarnessId,
    globallySelectedModelId,
    harnessDiscoveryPhase,
    page,
    sessionSelection,
    setChatHarnessSelection,
  ]);
  useLayoutEffect(() => {
    if (
      !chatId ||
      !chatSelection?.harnessId ||
      chatSelection.modelId !== null ||
      chatSelection.harnessId !== globallySelectedHarnessId ||
      harnessDiscoveryPhase !== "ready" ||
      globallySelectedModelId === null ||
      sessionSelection?.harnessId === chatSelection.harnessId
    )
      return;
    const ownership = captureChatOwnership(chatId);
    if (!ownership) return;
    setChatHarnessSelection(ownership, {
      harnessId: chatSelection.harnessId,
      modelId: globallySelectedModelId,
    });
  }, [
    captureChatOwnership,
    chatId,
    chatSelection?.harnessId,
    chatSelection?.modelId,
    globallySelectedHarnessId,
    globallySelectedModelId,
    harnessDiscoveryPhase,
    sessionSelection?.harnessId,
    setChatHarnessSelection,
  ]);
  const chooseHarness = useCallback(
    async (harnessId: string) => {
      const ownership = captureChatOwnership(chatId);
      if (!ownership) {
        await selectHarness(harnessId);
        return;
      }
      setChatHarnessSelection(ownership, { harnessId, modelId: null });
      await selectHarness(harnessId);
      const state = useWorkbenchStore.getState();
      if (state.selectedHarnessId !== harnessId) return;
      setChatHarnessSelection(ownership, {
        harnessId,
        modelId: state.selectedModelId,
      });
    },
    [captureChatOwnership, chatId, selectHarness, setChatHarnessSelection],
  );
  const chooseModel = useCallback(
    (modelId: string) => {
      const ownership = captureChatOwnership(chatId);
      if (!ownership) {
        selectModel(modelId);
        return;
      }
      selectModel(modelId);
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
    harnessDefinition,
    discoveryPhase,
    chooseHarness,
    chooseModel,
  };
}
