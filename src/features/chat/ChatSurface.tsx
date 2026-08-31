import { ArrowUp } from "lucide-react";
import { type FormEvent, useEffect, useRef, useState } from "react";

import { pinnedAgentChatOwner } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { SessionWorkingPill } from "./SessionWorkingPill";
import type {
  CuratedBindingId,
  CuratedModelId,
  TurnProjection,
} from "../../domain/acp";
import type { ResourceRef } from "../../domain/workbench";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { ChatLegacyTranscript } from "./ChatLegacyTranscript";
import { ChatHandoffControl } from "./ChatHandoffControl";
import { ChatHarnessControls } from "./ChatHarnessControls";
import {
  composerPromptPlaceholder,
  composerShortcutLabel,
  useComposerFocusHotkey,
} from "./composerChrome";
import { ComposerContext } from "./ComposerContext";
import { followEndScrollBehavior, transcriptIsAtEnd } from "./chatFollowEnd";
import { composerDisabledReason } from "./composerDisabledReason";
import { HarnessDiscoveryNotice } from "./HarnessDiscoveryNotice";
import {
  AcpChatProjection,
  type OpenResourceTarget,
  terminalTurnStates,
  useAcpChatActions,
  useAcpChatHistory,
  useAcpChatProjection,
  useAcpRetentionIncomplete,
  useRendererRecreationFocus,
} from "./acp";
import { currentNonterminalTurn } from "./acp/currentNonterminalTurn";
import { useBoundChatComposer } from "./useBoundChatComposer";

export function ChatSurface({ chatId }: { chatId?: string } = {}) {
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
  const openDiff = useWorkbenchStore((state) => state.openDiff);
  const openFile = useWorkbenchStore((state) => state.openFile);
  const selectedHarness = harnesses.find(
    (harness) => harness.id === selectedHarnessId,
  );
  const useNativeAcp = selectedHarness?.transport === "acp";
  const acpPage = useAcpChatProjection(useNativeAcp, chatId);
  const acpRetentionIncomplete = useAcpRetentionIncomplete(chatId);
  const acpActions = useAcpChatActions(chatId);
  const foregroundError = acpActions.error ?? chatError;
  const acpHistory = useAcpChatHistory(chatId);
  const surfaceRef = useRef<HTMLElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const [followingEnd, setFollowingEnd] = useState(true);
  const priorActiveTurnId = useRef<TurnProjection["id"] | null>(null);
  const terminalFocusTurnId = useRef<TurnProjection["id"] | null>(null);
  const composerFieldId = chatId ? `prompt-${chatId}` : "prompt";
  const composerDescriptionId = `${composerFieldId}-description`;
  const composerDisabledReasonId = `${composerFieldId}-disabled-reason`;
  const focused = chatId == null || pinnedAgentChatOwner() === chatId;
  useRendererRecreationFocus(surfaceRef, focused ? acpPage : null);

  const harnessAvailable = selectedHarness?.installState === "available";
  const selectedAcpBindingId =
    selectedHarness?.transport === "acp" && selectedHarness.bindingId
      ? (selectedHarness.bindingId as CuratedBindingId)
      : null;
  const currentAcpSession = acpPage?.sessions.find(
    (session) => session.id === acpPage.currentSessionId && !session.legacy,
  );
  const availableBinding = Boolean(
    useNativeAcp && harnessAvailable && selectedAcpBindingId,
  );
  const selectedCurrentSession =
    currentAcpSession &&
    currentAcpSession.bindingId === selectedAcpBindingId &&
    currentAcpSession.modelId === selectedModelId
      ? currentAcpSession
      : null;
  const harnessStatus = !selectedHarness
    ? "No harness"
    : harnessDiscoveryPhase === "checking"
      ? "Checking"
      : harnessDiscoveryPhase === "signInRequired"
        ? "Sign in required"
        : harnessDiscoveryPhase === "consent"
          ? "Sign-in consent"
          : harnessDiscoveryPhase === "waiting"
            ? "Waiting for sign in"
            : harnessDiscoveryPhase === "error"
              ? "Unavailable"
              : selectedHarness.source === "development"
                ? "Test simulator"
                : !harnessAvailable
                  ? selectedHarness.installState === "installing"
                    ? "Installing"
                    : "Unavailable"
                  : selectedCurrentSession?.state === "active"
                    ? "Live"
                    : selectedCurrentSession
                      ? `${selectedCurrentSession.state[0].toUpperCase()}${selectedCurrentSession.state.slice(1)}`
                      : "Ready";
  const actionBusy = useNativeAcp ? acpActions.busy : sending;
  const activeAcpTurn = useNativeAcp ? currentNonterminalTurn(acpPage) : null;
  const workingTurnCount = useNativeAcp
    ? (acpPage?.turns.filter((turn) => !terminalTurnStates.has(turn.state))
        .length ?? 0)
    : 0;
  const canCloseSession = Boolean(
    useNativeAcp &&
    currentAcpSession?.runtimeEpoch &&
    currentAcpSession.capabilities.closeSession &&
    !activeAcpTurn,
  );
  const canPrompt = useNativeAcp
    ? Boolean(
        acpPage &&
        harnessAvailable &&
        selectedAcpBindingId &&
        harnessDiscoveryPhase === "ready" &&
        (!selectedCurrentSession || selectedCurrentSession.capabilities.prompt),
      )
    : harnessAvailable;
  const composerEnabled = canPrompt && !activeAcpTurn;
  const composerHotkeyEnabled =
    focused && composerEnabled && !actionBusy && !projectTransitioning;
  useComposerFocusHotkey(composerRef, composerHotkeyEnabled);

  const disabledReason = composerDisabledReason({
    actionBusy,
    activeTurn: Boolean(activeAcpTurn),
    availableBinding,
    canPrompt,
    harnessAvailable,
    harnessDiscoveryPhase,
    projectTransitioning,
    selectedHarness: Boolean(selectedHarness),
    useNativeAcp,
  });

  useEffect(() => {
    if (!followingEnd) return;
    endRef.current?.scrollIntoView({
      behavior: followEndScrollBehavior(),
      block: "end",
    });
  }, [followingEnd, messages.length, acpPage?.projectionRevision]);

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
    if (terminalFocusTurnId.current && !actionBusy && !projectTransitioning) {
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
  }, [acpPage, activeAcpTurn, actionBusy, canPrompt, projectTransitioning]);

  function submit(event: FormEvent) {
    event.preventDefault();
    const bindingId = selectedAcpBindingId;
    if (useNativeAcp) {
      if (!bindingId || !composerEnabled || actionBusy || projectTransitioning)
        return;
      void acpActions.submit(
        composerText,
        bindingId,
        selectedModelId as CuratedModelId | null,
      );
      return;
    }
    void sendPrompt();
  }

  function openAcpResource(resource: ResourceRef, target: OpenResourceTarget) {
    void (target === "file"
      ? openFile(resource, "agent")
      : openDiff(resource, undefined, "agent"));
  }

  function updateFollowEnd() {
    const transcript = messageListRef.current;
    if (!transcript) return;
    setFollowingEnd(transcriptIsAtEnd(transcript));
  }

  function jumpToLatest() {
    setFollowingEnd(true);
    endRef.current?.scrollIntoView({
      behavior: followEndScrollBehavior(),
      block: "end",
    });
  }

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
              onClick={() => void acpActions.close()}
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

      <div
        ref={messageListRef}
        className="message-list"
        onScroll={updateFollowEnd}
      >
        {projectTransitioning ? (
          <div className="composer__notice" role="status">
            Loading the authoritative project…
          </div>
        ) : (
          <>
            {useNativeAcp && acpPage ? (
              <AcpChatProjection
                page={acpPage}
                active
                focused={focused}
                projectRoots={project?.roots ?? []}
                retentionIncomplete={acpRetentionIncomplete}
                onOpenResource={openAcpResource}
                hasEarlier={acpHistory.hasEarlier}
                loadingEarlier={acpHistory.loadingEarlier}
                historyError={acpHistory.historyError}
                onLoadEarlier={acpHistory.loadEarlier}
              />
            ) : (
              <ChatLegacyTranscript
                messages={messages}
                developmentSimulator={selectedHarness?.source === "development"}
              />
            )}
          </>
        )}
        <div ref={endRef} />
      </div>

      {!followingEnd ? (
        <button
          className="chat-follow-end"
          type="button"
          onClick={jumpToLatest}
        >
          Jump to latest
        </button>
      ) : null}

      <form
        className="composer"
        aria-busy={projectTransitioning}
        onSubmit={submit}
      >
        {error ? (
          <div className="composer__error" role="alert">
            {error}
          </div>
        ) : null}
        {foregroundError ? (
          <div className="composer__error" role="alert">
            {foregroundError}
          </div>
        ) : null}
        <HarnessDiscoveryNotice />
        {useNativeAcp &&
        !availableBinding &&
        !["checking", "signInRequired", "consent", "waiting", "error"].includes(
          harnessDiscoveryPhase,
        ) ? (
          <div className="composer__notice">
            No reviewed local binding is available for this harness.
          </div>
        ) : null}
        <label className="sr-only" htmlFor={composerFieldId}>
          Message agent
        </label>
        <span className="sr-only" id={composerDescriptionId}>
          Send a message to the selected harness and model
        </span>
        <textarea
          ref={composerRef}
          id={composerFieldId}
          aria-label="Message agent"
          aria-describedby={`${composerDescriptionId}${
            disabledReason ? ` ${composerDisabledReasonId}` : ""
          }`}
          value={composerText}
          maxLength={MAX_CHAT_COMPOSER_BYTES}
          rows={3}
          placeholder={composerPromptPlaceholder(
            canPrompt,
            composerShortcutLabel(/mac/i.test(navigator.platform)),
          )}
          disabled={!composerEnabled || actionBusy || projectTransitioning}
          onChange={(event) => setComposerText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && composerEnabled) {
              event.preventDefault();
              event.currentTarget.form?.requestSubmit();
            }
          }}
        />
        {disabledReason ? (
          <p
            className="composer__disabled-reason"
            id={composerDisabledReasonId}
          >
            {disabledReason}
          </p>
        ) : null}
        <div className="composer__toolbar">
          <ChatHarnessControls
            availableBinding={availableBinding}
            canPrompt={canPrompt}
          />
          <span className="composer__shortcut" aria-hidden="true">
            Enter to send <span>/</span> Shift+Enter for a new line
          </span>
          {useNativeAcp && acpActions.cancellableTurnId ? (
            <button
              className="send-button send-button--cancel"
              type="button"
              aria-label="Cancel turn"
              disabled={actionBusy || projectTransitioning}
              onClick={() =>
                void acpActions.cancel(acpActions.cancellableTurnId!)
              }
            >
              Cancel
            </button>
          ) : (
            <button
              className="send-button"
              type="submit"
              aria-label="Send message"
              disabled={
                projectTransitioning ||
                !composerText.trim() ||
                !composerEnabled ||
                actionBusy
              }
            >
              <ArrowUp size={18} aria-hidden="true" />
            </button>
          )}
        </div>
        <ComposerContext project={project} />
      </form>
    </section>
  );
}
