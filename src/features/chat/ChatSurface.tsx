import { useEffect, useRef } from "react";

import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import type { TurnProjection } from "../../domain/acp";
import { SessionWorkingPill } from "./SessionWorkingPill";
import { ChatHandoffControl } from "./ChatHandoffControl";
import { useComposerFocusHotkey } from "./composerChrome";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { ChatTranscript } from "./ChatTranscript";
import {
  terminalTurnStates,
  useAcpChatActions,
  useAcpChatHistory,
  useAcpChatProjection,
  useAcpRetentionIncomplete,
  useRendererRecreationFocus,
} from "./acp";
import { useBoundChatComposer } from "./useBoundChatComposer";
import { useChatSurfaceModel } from "./useChatSurfaceModel";
import { useFollowEnd } from "./useFollowEnd";

export function ChatSurface({ chatId }: { chatId?: string } = {}) {
  const focusedChatId = usePinnedAgentChatOwner();
  const messages = useWorkbenchStore((state) => state.messages);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const selectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const selectedModelId = useWorkbenchStore((state) => state.selectedModelId);
  const harnessDiscoveryPhase = useWorkbenchStore(
    (state) => state.harnessDiscoveryPhase,
  );
  const { composerText, sending, chatError, setComposerText, sendPrompt } =
    useBoundChatComposer(chatId);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const project = useWorkbenchStore((state) => state.project);
  const error = useWorkbenchStore((state) => state.error);
  // This preliminary selector only chooses the ACP subscription.  All
  // presentation/action derivation lives in the named model below.
  const selectedTransport = harnesses.find(
    (harness) => harness.id === selectedHarnessId,
  )?.transport;
  const acpPage = useAcpChatProjection(selectedTransport === "acp", chatId);
  const acpRetentionIncomplete = useAcpRetentionIncomplete(chatId);
  const acpActions = useAcpChatActions(chatId);
  const acpHistory = useAcpChatHistory(chatId);
  const surfaceRef = useRef<HTMLElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const priorActiveTurnId = useRef<TurnProjection["id"] | null>(null);
  const terminalFocusTurnId = useRef<TurnProjection["id"] | null>(null);
  const composerFieldId = chatId ? `prompt-${chatId}` : "prompt";
  const composerDescriptionId = `${composerFieldId}-description`;
  const composerDisabledReasonId = `${composerFieldId}-disabled-reason`;
  const focused = chatId == null || focusedChatId === chatId;
  const {
    selectedHarness,
    useNativeAcp,
    availableBinding,
    harnessStatus,
    foregroundError,
    actionBusy,
    activeAcpTurn,
    workingTurnCount,
    canCloseSession,
    canPrompt,
    usage,
    composerEnabled,
    composerHotkeyEnabled,
    disabledReason,
    submit,
    closeSession,
    cancelTurn,
    openResource,
  } = useChatSurfaceModel({
    harnesses,
    selectedHarnessId,
    selectedModelId,
    page: acpPage,
    discoveryPhase: harnessDiscoveryPhase,
    composerText,
    sending,
    chatError,
    projectTransitioning,
    focused,
    acpActions,
    sendPrompt,
  });
  useRendererRecreationFocus(surfaceRef, focused ? acpPage : null);

  useComposerFocusHotkey(composerRef, composerHotkeyEnabled);

  const { followingEnd, updateFollowEnd, jumpToLatest } = useFollowEnd({
    ownerKey: chatId ?? "primary",
    revision: `${messages.length}:${acpPage?.projectionRevision ?? ""}`,
    transcriptRef: messageListRef,
    endRef,
  });

  useEffect(() => {
    const priorId = priorActiveTurnId.current;
    const priorTurn = priorId
      ? acpPage?.turns.find((turn) => turn.id === priorId)
      : null;
    if (priorId && priorTurn && terminalTurnStates.has(priorTurn.state)) {
      terminalFocusTurnId.current = priorId;
      priorActiveTurnId.current = activeAcpTurn?.id ?? null;
    } else if (activeAcpTurn) {
      priorActiveTurnId.current = activeAcpTurn.id;
    } else if (priorId && !priorTurn) {
      priorActiveTurnId.current = null;
    }
    if (
      focused &&
      terminalFocusTurnId.current &&
      !actionBusy &&
      !projectTransitioning
    ) {
      const composer = composerRef.current;
      const statuses =
        surfaceRef.current?.querySelectorAll<HTMLElement>("[data-turn-status]");
      const terminalStatus = [...(statuses ?? [])].find(
        (status) => status.dataset.turnStatus === terminalFocusTurnId.current,
      );
      const target = composer && !composer.disabled ? composer : terminalStatus;
      target?.focus();
      if (target) terminalFocusTurnId.current = null;
    }
  }, [
    acpPage,
    activeAcpTurn,
    actionBusy,
    canPrompt,
    focused,
    projectTransitioning,
  ]);

  return (
    <section ref={surfaceRef} className="chat-surface" aria-label="Agent chat">
      <header className="surface-header">
        <SessionWorkingPill
          count={workingTurnCount}
          hidden={projectTransitioning}
        />
        <div className="session-controls">
          <ChatHandoffControl
            acpPage={useNativeAcp ? acpPage : null}
            messages={messages}
            busy={actionBusy || Boolean(activeAcpTurn)}
            onCommitted={() =>
              queueMicrotask(() => composerRef.current?.focus())
            }
          />
          {canCloseSession ? (
            <button
              className="button button--compact"
              type="button"
              aria-label="Close session"
              disabled={actionBusy || projectTransitioning}
              onClick={closeSession}
            >
              Close session
            </button>
          ) : null}
          <div
            className="session-state"
            aria-label={`Harness status: ${harnessStatus}`}
          >
            {harnessStatus === "Live" ? (
              <span className="session-state__dot" />
            ) : null}
            {harnessStatus}
          </div>
        </div>
      </header>

      <ChatTranscript
        transcriptRef={messageListRef}
        endRef={endRef}
        onScroll={updateFollowEnd}
        projectTransitioning={projectTransitioning}
        useNativeAcp={useNativeAcp}
        page={acpPage}
        focused={focused}
        projectRoots={project?.roots ?? []}
        retentionIncomplete={acpRetentionIncomplete}
        onOpenResource={openResource}
        hasEarlier={acpHistory.hasEarlier}
        loadingEarlier={acpHistory.loadingEarlier}
        historyError={acpHistory.historyError}
        onLoadEarlier={acpHistory.loadEarlier}
        messages={messages}
        developmentSimulator={selectedHarness?.source === "development"}
        followingEnd={followingEnd}
        onJumpToLatest={jumpToLatest}
      />

      <ChatComposerRegion
        composerRef={composerRef}
        fieldId={composerFieldId}
        descriptionId={composerDescriptionId}
        disabledReasonId={composerDisabledReasonId}
        text={composerText}
        enabled={composerEnabled}
        actionBusy={actionBusy}
        projectTransitioning={projectTransitioning}
        workbenchError={error}
        chatError={foregroundError}
        availableBinding={availableBinding}
        canPrompt={canPrompt}
        missingBindingNotice={
          useNativeAcp &&
          !availableBinding &&
          ![
            "checking",
            "signInRequired",
            "consent",
            "waiting",
            "error",
          ].includes(harnessDiscoveryPhase)
        }
        disabledReason={disabledReason}
        project={project}
        usage={usage}
        onSubmit={submit}
        onTextChange={setComposerText}
        onCancelTurn={cancelTurn}
      />
    </section>
  );
}
