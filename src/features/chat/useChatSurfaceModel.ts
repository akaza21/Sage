import { type FormEvent, useCallback, useEffect, useMemo, useRef } from "react";

import { openAgentArtifact } from "../../app/openAgentArtifact";
import type { AgentDiffRef } from "../../domain/agentDiff";
import type { DiffLineCommentContext } from "../../domain/attachedContext";
import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import type {
  CuratedBindingId,
  CuratedModelId,
  TurnProjection,
} from "../../domain/acp";
import type { AcpChatPage } from "../../domain/acp";
import type { ResourceRef } from "../../domain/workbench";
import { composerDisabledReason } from "./composerDisabledReason";
import { currentNonterminalTurn } from "./acp/currentNonterminalTurn";
import { currentChatUsage } from "./acp/chatUsage";
import { terminalTurnStates, type OpenResourceTarget } from "./acp";

export function useChatSurfaceModel({
  harnesses,
  selectedHarnessId,
  selectedModelId,
  page,
  discoveryPhase,
  composerText = "",
  attachedContext = [],
  sending = false,
  chatError = null,
  projectTransitioning = false,
  focused = false,
  acpActions,
  sendPrompt,
}: {
  harnesses: readonly {
    id: string;
    transport: string;
    bindingId?: string | null;
    installState: string;
    source: string;
  }[];
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  page: AcpChatPage | null;
  discoveryPhase: HarnessDiscoveryPhase;
  composerText: string;
  attachedContext: readonly DiffLineCommentContext[];
  sending: boolean;
  chatError: string | null;
  projectTransitioning: boolean;
  focused: boolean;
  acpActions: {
    busy: boolean;
    error: string | null;
    modeError?: string | null;
    modeChanging?: boolean;
    cancellableTurnId: TurnProjection["id"] | null;
    submit: (
      text: string,
      attachedContext: readonly DiffLineCommentContext[],
      bindingId: CuratedBindingId,
      modelId: CuratedModelId | null,
    ) => Promise<unknown>;
    cancel: (turnId: TurnProjection["id"]) => Promise<unknown>;
    close: () => Promise<unknown>;
  };
  sendPrompt: () => Promise<unknown>;
}) {
  const model = useMemo(() => {
    const selectedHarness = harnesses.find(
      (harness) => harness.id === selectedHarnessId,
    );
    const useNativeAcp = selectedHarness?.transport === "acp";
    const bindingId =
      useNativeAcp && selectedHarness?.bindingId
        ? (selectedHarness.bindingId as CuratedBindingId)
        : null;
    const session = page?.sessions.find(
      (candidate) =>
        candidate.id === page.currentSessionId && !candidate.legacy,
    );
    const selectedSession =
      session &&
      session.bindingId === bindingId &&
      session.modelId === selectedModelId
        ? session
        : null;
    const harnessAvailable = selectedHarness?.installState === "available";
    const availableBinding = Boolean(
      useNativeAcp && harnessAvailable && bindingId,
    );
    const harnessStatus = !selectedHarness
      ? "No harness"
      : discoveryPhase === "checking"
        ? "Checking"
        : discoveryPhase === "signInRequired"
          ? "Sign in required"
          : discoveryPhase === "consent"
            ? "Sign-in consent"
            : discoveryPhase === "waiting"
              ? "Waiting for sign in"
              : discoveryPhase === "error"
                ? "Unavailable"
                : selectedHarness.source === "development"
                  ? "Test simulator"
                  : !harnessAvailable
                    ? selectedHarness.installState === "installing"
                      ? "Installing"
                      : "Unavailable"
                    : selectedSession?.state === "active"
                      ? "Live"
                      : selectedSession
                        ? `${selectedSession.state[0].toUpperCase()}${selectedSession.state.slice(1)}`
                        : "Ready";
    return {
      selectedHarness,
      useNativeAcp: Boolean(useNativeAcp),
      bindingId,
      selectedSession,
      selectedModelId: selectedModelId as CuratedModelId | null,
      harnessAvailable,
      availableBinding,
      harnessStatus,
    };
  }, [discoveryPhase, harnesses, page, selectedHarnessId, selectedModelId]);
  const pageRef = useRef(page);
  useEffect(() => {
    pageRef.current = page;
  }, [page]);
  const turnEligibility = useMemo(() => {
    const activeAcpTurn = model.useNativeAcp
      ? currentNonterminalTurn(page)
      : null;
    const workingTurnCount = model.useNativeAcp
      ? (page?.turns.filter((turn) => !terminalTurnStates.has(turn.state))
          .length ?? 0)
      : 0;
    const currentSession = page?.sessions.find(
      (session) => session.id === page.currentSessionId && !session.legacy,
    );
    const canCloseSession = Boolean(
      model.useNativeAcp &&
      currentSession?.runtimeEpoch &&
      currentSession.capabilities.closeSession &&
      !activeAcpTurn,
    );
    const canPrompt = model.useNativeAcp
      ? Boolean(
          page &&
          model.harnessAvailable &&
          model.bindingId &&
          discoveryPhase === "ready" &&
          (!model.selectedSession || model.selectedSession.capabilities.prompt),
        )
      : model.harnessAvailable;
    const usage = model.useNativeAcp ? currentChatUsage(page) : null;
    return {
      activeAcpTurn,
      workingTurnCount,
      canCloseSession,
      canPrompt,
      usage,
    };
  }, [
    discoveryPhase,
    model.bindingId,
    model.harnessAvailable,
    model.selectedSession,
    model.useNativeAcp,
    page,
  ]);
  const { activeAcpTurn, workingTurnCount, canCloseSession, canPrompt, usage } =
    turnEligibility;
  const actionBusy = model.useNativeAcp ? acpActions.busy : sending;
  const legacyAttachments = !model.useNativeAcp && attachedContext.length > 0;
  const composerEnabled = canPrompt && !activeAcpTurn && !legacyAttachments;
  const disabledReason = legacyAttachments
    ? "Attached diff comments require an ACP harness. Remove them or select an ACP harness."
    : composerDisabledReason({
        actionBusy,
        activeTurn: Boolean(activeAcpTurn),
        availableBinding: model.availableBinding,
        canPrompt,
        harnessAvailable: model.harnessAvailable,
        harnessDiscoveryPhase: discoveryPhase,
        projectTransitioning,
        selectedHarness: Boolean(model.selectedHarness),
        useNativeAcp: model.useNativeAcp,
      });
  const submit = useCallback(
    (event: FormEvent) => {
      event.preventDefault();
      if (model.useNativeAcp) {
        if (
          !model.bindingId ||
          !composerEnabled ||
          actionBusy ||
          projectTransitioning
        )
          return;
        void acpActions.submit(
          composerText,
          attachedContext,
          model.bindingId,
          model.selectedModelId,
        );
        return;
      }
      if (legacyAttachments) return;
      void sendPrompt();
    },
    [
      acpActions,
      actionBusy,
      attachedContext,
      composerEnabled,
      composerText,
      legacyAttachments,
      model.bindingId,
      model.selectedModelId,
      model.useNativeAcp,
      projectTransitioning,
      sendPrompt,
    ],
  );
  const openResource = useCallback(
    (
      resource: ResourceRef,
      target: OpenResourceTarget,
      origin?: {
        projectId: AcpChatPage["projectId"];
        chatId: AcpChatPage["chatId"];
        sessionId: NonNullable<AcpChatPage["currentSessionId"]>;
      },
      diffRef?: AgentDiffRef,
    ) => {
      const current = pageRef.current;
      if (
        !origin ||
        !current?.currentSessionId ||
        current.projectId !== origin.projectId ||
        current.chatId !== origin.chatId ||
        current.currentSessionId !== origin.sessionId
      )
        return;
      void openAgentArtifact({
        owner: origin,
        target,
        resource,
        ...(diffRef ? { diffRef } : {}),
      });
    },
    [],
  );
  return {
    ...model,
    foregroundError: acpActions.error ?? chatError,
    modeError: model.useNativeAcp ? (acpActions.modeError ?? null) : null,
    modeChanging: model.useNativeAcp
      ? (acpActions.modeChanging ?? false)
      : false,
    actionBusy,
    activeAcpTurn,
    workingTurnCount,
    canCloseSession,
    canPrompt,
    usage,
    composerEnabled,
    composerHotkeyEnabled:
      focused && composerEnabled && !actionBusy && !projectTransitioning,
    disabledReason,
    submit,
    closeSession: () => void acpActions.close(),
    cancelTurn:
      model.useNativeAcp && acpActions.cancellableTurnId
        ? () => void acpActions.cancel(acpActions.cancellableTurnId!)
        : null,
    openResource,
  };
}
