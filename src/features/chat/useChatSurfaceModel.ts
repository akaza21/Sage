import { useCallback, useEffect, useMemo, useRef, type FormEvent } from "react";

import type { HarnessDiscoveryPhase } from "../../app/harnessSlice";
import { openAgentArtifact } from "../../app/openAgentArtifact";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import type {
  AcpChatPage,
  AcpCommandFailure,
  CuratedBindingId,
  CuratedModelId,
  TurnProjection,
} from "../../domain/acp";
import type { AgentDiffRef } from "../../domain/agentDiff";
import type { AttachedContext } from "../../domain/attachedContext";
import { MAX_QUEUED_CHAT_PROMPTS } from "../../domain/chatOwnership";
import type { HarnessDefinition } from "../../domain/harness";
import type { ResourceRef } from "../../domain/workbench";
import { terminalTurnStates, type OpenResourceTarget } from "./acp";
import { currentChatUsage } from "./acp/chatUsage";
import { currentNonterminalTurn } from "./acp/currentNonterminalTurn";
import { composerDisabledReason, sizeReason } from "./composerDisabledReason";

type SurfaceHarness = Pick<
  HarnessDefinition,
  | "id"
  | "transport"
  | "bindingId"
  | "installState"
  | "source"
  | "name"
  | "signIn"
  | "accessModes"
>;

export function useChatSurfaceModel({
  harnesses,
  selectedHarnessId,
  selectedModelId,
  page,
  discoveryPhase,
  composerText = "",
  attachedContext = [],
  imageInputAvailable = false,
  imageUploading = false,
  sending = false,
  chatError = null,
  chatLoadFailed = false,
  projectTransitioning = false,
  focused = false,
  acpActions,
  sendPrompt,
  queuePrompt,
  queuedPromptCount = 0,
  queueSavePending = false,
}: {
  harnesses: readonly SurfaceHarness[];
  selectedHarnessId: string | null;
  selectedModelId: string | null;
  page: AcpChatPage | null;
  discoveryPhase: HarnessDiscoveryPhase;
  composerText: string;
  attachedContext: readonly AttachedContext[];
  imageInputAvailable?: boolean;
  imageUploading?: boolean;
  sending: boolean;
  chatError: string | null;
  chatLoadFailed?: boolean;
  projectTransitioning: boolean;
  focused: boolean;
  acpActions: {
    busy: boolean;
    error: string | null;
    errorKind?: AcpCommandFailure["kind"] | null;
    modeError?: string | null;
    modeChanging?: boolean;
    cancellableTurnId: TurnProjection["id"] | null;
    submit: (
      text: string,
      attachedContext: readonly AttachedContext[],
      bindingId: CuratedBindingId,
      modelId: CuratedModelId | null,
    ) => Promise<unknown>;
    cancel: (turnId: TurnProjection["id"]) => Promise<unknown>;
    close: () => Promise<unknown>;
    end: () => Promise<unknown>;
  };
  sendPrompt: () => Promise<unknown>;
  queuePrompt?: (bindingId: string, modelId: string | null) => Promise<boolean>;
  queuedPromptCount?: number;
  queueSavePending?: boolean;
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
    const sessionHarnessId =
      session?.runtimeEpoch &&
      !["closed", "failed", "incompatible"].includes(session.state)
        ? (harnesses.find(
            (harness) =>
              harness.transport === "acp" &&
              harness.bindingId === session.bindingId,
          )?.id ?? null)
        : null;
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
      sessionHarnessId,
      sessionModelId: session?.modelId ?? null,
      selectedModelId: selectedModelId as CuratedModelId | null,
      harnessAvailable,
      availableBinding,
      harnessStatus,
    };
  }, [discoveryPhase, harnesses, page, selectedHarnessId, selectedModelId]);
  const accessBindingId = model.selectedSession?.bindingId ?? model.bindingId;
  const access = useSessionAccessSelection(
    accessBindingId,
    model.selectedHarness?.accessModes,
  );
  const accessReason = model.availableBinding ? access.reason : null;
  const pageRef = useRef(page);
  useEffect(() => {
    pageRef.current = page;
  }, [page]);
  const turnEligibility = useMemo(() => {
    const activeAcpTurn = model.useNativeAcp
      ? currentNonterminalTurn(page)
      : null;
    const workingTurnCount =
      model.useNativeAcp && page?.currentSessionId
        ? page.turns.filter(
            (turn) =>
              turn.sessionId === page.currentSessionId &&
              !terminalTurnStates.has(turn.state),
          ).length
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
    const canEndSession = Boolean(
      model.useNativeAcp &&
      currentSession?.runtimeEpoch &&
      !currentSession.capabilities.closeSession,
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
    const usage =
      model.useNativeAcp && model.selectedSession
        ? currentChatUsage(page)
        : null;
    return {
      activeAcpTurn,
      workingTurnCount,
      canCloseSession,
      canEndSession,
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
  const {
    activeAcpTurn,
    workingTurnCount,
    canCloseSession,
    canEndSession,
    canPrompt: protocolCanPrompt,
    usage,
  } = turnEligibility;
  const canPrompt = protocolCanPrompt && !accessReason;
  const actionBusy = model.useNativeAcp ? acpActions.busy : sending;
  const legacyAttachments = !model.useNativeAcp && attachedContext.length > 0;
  const sendBlocked =
    queueSavePending ||
    (model.useNativeAcp && chatLoadFailed) ||
    sizeReason(composerText, attachedContext) !== null ||
    imageUploading ||
    (attachedContext.some((context) => context.kind === "image") &&
      !imageInputAvailable);
  const queueAvailable = Boolean(
    model.useNativeAcp &&
    activeAcpTurn?.state === "running" &&
    canPrompt &&
    !actionBusy &&
    !projectTransitioning &&
    !queueSavePending &&
    queuedPromptCount < MAX_QUEUED_CHAT_PROMPTS,
  );
  const composerEnabled =
    canPrompt && !(model.useNativeAcp && chatLoadFailed) && !legacyAttachments;
  const sessionPromptUnavailable = Boolean(
    model.useNativeAcp &&
    model.selectedSession &&
    !model.selectedSession.capabilities.prompt,
  );
  const disabledReason =
    accessReason ??
    sizeReason(composerText, attachedContext) ??
    (legacyAttachments
      ? "Attached context requires an ACP harness. Remove it or select an ACP harness."
      : queueSavePending
        ? "Saving queued message."
        : activeAcpTurn && queuedPromptCount >= MAX_QUEUED_CHAT_PROMPTS
          ? `Queue is full (${MAX_QUEUED_CHAT_PROMPTS} messages).`
          : queueAvailable
            ? null
            : composerDisabledReason({
                actionBusy,
                activeTurn: Boolean(activeAcpTurn),
                availableBinding: model.availableBinding,
                canPrompt,
                chatLoadFailed: model.useNativeAcp && chatLoadFailed,
                harnessAvailable: model.harnessAvailable,
                harnessDiscoveryPhase: discoveryPhase,
                projectTransitioning,
                selectedHarness: Boolean(model.selectedHarness),
                useNativeAcp: model.useNativeAcp,
              }));
  const submit = useCallback(
    (event: FormEvent) => {
      event.preventDefault();
      if (sendBlocked) return;
      if (activeAcpTurn) {
        if (queueAvailable && model.bindingId)
          void queuePrompt?.(model.bindingId, model.selectedModelId);
        return;
      }
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
      activeAcpTurn,
      actionBusy,
      attachedContext,
      composerEnabled,
      composerText,
      legacyAttachments,
      sendBlocked,
      model.bindingId,
      model.selectedModelId,
      model.useNativeAcp,
      projectTransitioning,
      queueAvailable,
      queuePrompt,
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
        select: true,
        resource,
        ...(diffRef ? { diffRef } : {}),
      });
    },
    [],
  );
  return {
    ...model,
    accessBindingId,
    foregroundError:
      acpActions.errorKind === "signInRequired" && model.selectedHarness
        ? signInRequiredMessage(model.selectedHarness)
        : (acpActions.error ?? chatError),
    modeError: model.useNativeAcp ? (acpActions.modeError ?? null) : null,
    modeChanging: model.useNativeAcp && (acpActions.modeChanging ?? false),
    actionBusy,
    activeAcpTurn,
    workingTurnCount,
    canCloseSession,
    canEndSession,
    canPrompt,
    sessionPromptUnavailable,
    usage,
    composerEnabled,
    sendBlocked,
    queueAvailable,
    composerHotkeyEnabled:
      focused && composerEnabled && !actionBusy && !projectTransitioning,
    disabledReason,
    submit,
    closeSession: () => void acpActions.close(),
    endSession: () => void acpActions.end(),
    cancelTurn:
      model.useNativeAcp && acpActions.cancellableTurnId
        ? () => void acpActions.cancel(acpActions.cancellableTurnId!)
        : null,
    steerAvailable: Boolean(
      model.useNativeAcp &&
      activeAcpTurn &&
      page?.sessions.find((session) => session.id === page.currentSessionId)
        ?.capabilities?.activeTurnSteer,
    ),
    openResource,
  };
}

function signInRequiredMessage(harness: SurfaceHarness): string {
  if (harness.signIn) {
    return `${harness.name} requires sign-in before it can continue. Run ${harness.signIn.command}. ${harness.signIn.hint} Then try again.`;
  }
  return `${harness.name} requires sign-in before it can continue. Sign in with its CLI, then try again.`;
}
