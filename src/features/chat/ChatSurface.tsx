import { useContext, useEffect, useRef, type ReactNode } from "react";
import {
  AgentSessionChromeContext,
  usePinnedAgentChatOwner,
} from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import type { TurnProjection } from "../../domain/acp";
import { SessionWorkingPill } from "./SessionWorkingPill";
import { ChatHandoffControl } from "./ChatHandoffControl";
import { useComposerFocusHotkey } from "./composerChrome";
import { ChatComposerRegion } from "./ChatComposerRegion";
import { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";
import { ChatTranscript } from "./ChatTranscript";
import {
  terminalTurnStates,
  permissionRequestsAvailable,
  latestRetryPrompt,
  useAcpChatActions,
  useAcpChatHistory,
  useAcpRetentionIncomplete,
  useAcpChatLoadFailure,
  useRendererRecreationFocus,
} from "./acp";
import {
  useBoundChatComposer,
  useQueuedPromptDrain,
} from "./useBoundChatComposer";
import { useChatHarnessSelection } from "./useChatHarnessSelection";
import { useChatSurfaceModel } from "./useChatSurfaceModel";
import { useFollowEnd } from "./useFollowEnd";
import { bindingNoticeAllowed } from "./composerBindingNotice";
export function ChatSurface({
  chatId,
  paneHeader,
}: { chatId?: string; paneHeader?: ReactNode } = {}) {
  const focusedChatId = usePinnedAgentChatOwner();
  const setAgentSessionChrome = useContext(AgentSessionChromeContext);
  const messages = useWorkbenchStore((state) => state.messages);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const {
    page: acpPage,
    selectedHarnessId,
    selectedModelId,
    harnessDefinition: selectedHarnessDefinition,
    discoveryPhase,
    chooseHarness,
    chooseModel,
  } = useChatHarnessSelection(chatId);
  const {
    composerText,
    attachedContext,
    imagePreviews,
    imageUploading,
    addImages,
    queuedPrompts,
    queuedTurnBarrier,
    knownTurnOrder,
    sending,
    chatError,
    setComposerText,
    removeAttachedContext,
    sendPrompt,
    queuePrompt,
    updateQueuedPrompt,
    steerQueuedPrompt,
    setQueuedTurnBarrier,
    rememberTurnOrder,
  } = useBoundChatComposer(chatId);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const project = useWorkbenchStore((state) => state.project);
  const error = useWorkbenchStore((state) => state.error);
  const acpRetentionIncomplete = useAcpRetentionIncomplete(chatId);
  const acpLoadFailure = useAcpChatLoadFailure(chatId);
  const acpActions = useAcpChatActions(chatId);
  const acpHistory = useAcpChatHistory(chatId);
  const surfaceRef = useRef<HTMLElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);
  const transcriptContentRef = useRef<HTMLDivElement>(null);
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
    modeError,
    modeChanging,
    actionBusy,
    activeAcpTurn,
    workingTurnCount,
    canCloseSession,
    canEndSession,
    canPrompt,
    sessionHarnessId,
    sessionModelId,
    sessionPromptUnavailable,
    usage,
    composerEnabled,
    queueAvailable,
    composerHotkeyEnabled,
    disabledReason,
    submit,
    closeSession,
    endSession,
    cancelTurn,
    steerAvailable,
    openResource,
  } = useChatSurfaceModel({
    harnesses,
    selectedHarnessId,
    selectedModelId,
    page: acpPage,
    discoveryPhase,
    composerText,
    attachedContext,
    imageUploading,
    sending,
    chatError,
    chatLoadFailed: Boolean(acpLoadFailure.error),
    projectTransitioning,
    focused,
    acpActions,
    sendPrompt,
    queuePrompt,
    queuedPromptCount: queuedPrompts.length,
    queueSavePending: queuedPrompts.some(
      (prompt) => prompt.status === "saving",
    ),
  });
  useQueuedPromptDrain({
    queuedPrompts,
    queuedTurnBarrier,
    knownTurnOrder,
    activeTurn: Boolean(activeAcpTurn),
    actionBusy,
    projectTransitioning,
    canPrompt,
    useNativeAcp,
    page: acpPage ?? null,
    submit: acpActions.submit,
    updateQueuedPrompt,
    setQueuedTurnBarrier,
    rememberTurnOrder,
  });
  const sessionActions = useRef({
    closeSession,
    endSession,
    focusComposer: () => composerRef.current?.focus(),
  });
  useEffect(() => {
    sessionActions.current = {
      closeSession,
      endSession,
      focusComposer: () => composerRef.current?.focus(),
    };
  }, [closeSession, endSession]);
  useEffect(() => {
    if (!setAgentSessionChrome || !chatId || !focused) return;
    setAgentSessionChrome({
      chatId,
      harnessStatus,
      canCloseSession,
      canEndSession,
      busy: actionBusy || Boolean(activeAcpTurn) || projectTransitioning,
      acpPage: useNativeAcp ? acpPage : null,
      messages,
      actions: sessionActions,
    });
    return () => setAgentSessionChrome(null);
  }, [
    setAgentSessionChrome,
    chatId,
    focused,
    harnessStatus,
    canCloseSession,
    canEndSession,
    actionBusy,
    activeAcpTurn,
    projectTransitioning,
    useNativeAcp,
    acpPage,
    messages,
  ]);
  useRendererRecreationFocus(surfaceRef, focused ? acpPage : null);
  useComposerFocusHotkey(composerRef, composerHotkeyEnabled);
  const { showJumpToLatest, updateFollowEnd, jumpToLatest } = useFollowEnd({
    ownerKey: chatId ?? "primary",
    revision: `${messages.length}:${acpPage?.projectionRevision ?? ""}`,
    transcriptRef: messageListRef,
    contentRef: transcriptContentRef,
    endRef,
  });
  const retryLastTurn = () => {
    if (!acpPage || actionBusy || activeAcpTurn) return;
    const retry = latestRetryPrompt(acpPage);
    if (retry)
      void acpActions.submit(retry.prompt, [], retry.bindingId, retry.modelId);
  };

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
    <section
      ref={surfaceRef}
      className={`chat-surface${setAgentSessionChrome ? " chat-surface--agent" : ""}`}
      aria-label="Agent chat"
    >
      {!setAgentSessionChrome ? (
        <header className="surface-header">
          {paneHeader}
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
            {canEndSession ? (
              <button
                className="button button--compact"
                type="button"
                aria-label="End session"
                disabled={actionBusy || projectTransitioning}
                onClick={endSession}
              >
                End session
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
      ) : null}
      <HarnessDiscoveryNotice
        harness={selectedHarnessDefinition}
        discoveryPhase={discoveryPhase}
      />

      <ChatTranscript
        transcriptRef={messageListRef}
        contentRef={transcriptContentRef}
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
        onRetry={
          useNativeAcp && !projectTransitioning && !actionBusy && !activeAcpTurn
            ? retryLastTurn
            : undefined
        }
        showJumpToLatest={showJumpToLatest}
        onJumpToLatest={jumpToLatest}
      />

      <ChatComposerRegion
        composerRef={composerRef}
        fieldId={composerFieldId}
        dictationScope={chatId ?? activeTabId ?? ""}
        descriptionId={composerDescriptionId}
        disabledReasonId={composerDisabledReasonId}
        text={composerText}
        attachedContext={attachedContext}
        imagePreviews={imagePreviews}
        imageUploading={imageUploading}
        onAddImages={addImages}
        enabled={composerEnabled}
        actionBusy={actionBusy}
        activeTurn={Boolean(activeAcpTurn)}
        queueAvailable={queueAvailable}
        queuedPrompts={queuedPrompts}
        steerAvailable={steerAvailable}
        projectTransitioning={projectTransitioning}
        workbenchError={error}
        chatError={foregroundError}
        claudeSignInRequired={
          selectedHarnessId === "claude-code" &&
          acpActions.errorKind === "signInRequired"
        }
        claudeAuthScope={`${project?.id ?? ""}:${epoch}:${chatId ?? activeTabId ?? ""}`}
        availableBinding={availableBinding}
        canPrompt={canPrompt}
        discoveryPhase={discoveryPhase}
        sessionHarnessId={sessionHarnessId}
        sessionModelId={sessionModelId}
        selectedHarnessId={selectedHarnessId}
        selectedModelId={selectedModelId}
        sessionPromptUnavailable={sessionPromptUnavailable}
        sessionConfig={acpPage?.sessionConfig ?? null}
        permissionRequests={permissionRequestsAvailable(
          useNativeAcp ? (acpPage ?? null) : null,
          selectedHarnessDefinition,
        )}
        modePending={modeChanging}
        modeError={modeError}
        focused={focused}
        onSetSessionMode={acpActions.setSessionMode}
        onSetSessionConfigOption={acpActions.setSessionConfigOption}
        onSelectHarness={chooseHarness}
        onSelectModel={chooseModel}
        missingBindingNotice={
          useNativeAcp &&
          !availableBinding &&
          bindingNoticeAllowed(discoveryPhase)
        }
        disabledReason={disabledReason}
        onRetryLoad={acpLoadFailure.error ? acpLoadFailure.retry : null}
        project={project}
        usage={usage}
        goal={(useNativeAcp && acpPage?.goal) || { kind: "unsupported" }}
        onSubmit={submit}
        onTextChange={setComposerText}
        onRemoveContext={removeAttachedContext}
        onCancelTurn={cancelTurn}
        onDeleteQueuedPrompt={(id) => updateQueuedPrompt(id, "deleted")}
        onRetryQueuedPrompt={(id) => updateQueuedPrompt(id, "queued")}
        onSteerQueuedPrompt={(id) => steerQueuedPrompt(id, acpActions.steer)}
      />
    </section>
  );
}
