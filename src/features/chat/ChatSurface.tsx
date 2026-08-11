import { ArrowUp, Check, ChevronDown, ShieldCheck } from "lucide-react";
import { type FormEvent, useEffect, useRef } from "react";

import { useWorkbenchStore } from "../../app/store";
import type {
  AcpChatPage,
  CuratedBindingId,
  CuratedModelId,
  TurnProjection,
} from "../../domain/acp";
import type { ResourceRef } from "../../domain/workbench";
import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { EditSummary } from "./EditSummary";
import { ChatHandoffControl } from "./ChatHandoffControl";
import {
  AcpChatProjection,
  type OpenResourceTarget,
  terminalTurnStates,
  useAcpChatActions,
  useAcpChatHistory,
  useAcpChatProjection,
  useRendererRecreationFocus,
} from "./acp";

function currentNonterminalTurn(
  page: AcpChatPage | null,
): TurnProjection | null {
  let current: TurnProjection | null = null;
  for (const turn of page?.turns ?? []) {
    if (
      !terminalTurnStates.has(turn.state) &&
      (!current || turn.ordinal > current.ordinal)
    ) {
      current = turn;
    }
  }
  return current;
}

export function ChatSurface() {
  const messages = useWorkbenchStore((state) => state.messages);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const selectedHarnessId = useWorkbenchStore(
    (state) => state.selectedHarnessId,
  );
  const models = useWorkbenchStore((state) => state.models);
  const selectedModelId = useWorkbenchStore((state) => state.selectedModelId);
  const composerText = useWorkbenchStore((state) => state.composerText);
  const sending = useWorkbenchStore((state) => state.sending);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const error = useWorkbenchStore((state) => state.error);
  const chatError = useWorkbenchStore((state) => state.chatError);
  const selectHarness = useWorkbenchStore((state) => state.selectHarness);
  const selectModel = useWorkbenchStore((state) => state.selectModel);
  const setComposerText = useWorkbenchStore((state) => state.setComposerText);
  const sendPrompt = useWorkbenchStore((state) => state.sendPrompt);
  const openDiff = useWorkbenchStore((state) => state.openDiff);
  const openFile = useWorkbenchStore((state) => state.openFile);
  const selectedHarness = harnesses.find(
    (harness) => harness.id === selectedHarnessId,
  );
  const useNativeAcp = selectedHarness?.transport === "acp";
  const acpPage = useAcpChatProjection(useNativeAcp);
  const acpActions = useAcpChatActions();
  const foregroundError = acpActions.error ?? chatError;
  const acpHistory = useAcpChatHistory();
  const surfaceRef = useRef<HTMLElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const priorActiveTurnId = useRef<TurnProjection["id"] | null>(null);
  const terminalFocusTurnId = useRef<TurnProjection["id"] | null>(null);
  useRendererRecreationFocus(surfaceRef, acpPage);

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
  const harnessStatus = !selectedHarness
    ? "No harness"
    : selectedHarness.source === "development"
      ? "Test simulator"
      : !harnessAvailable
        ? selectedHarness.installState === "installing"
          ? "Installing"
          : "Unavailable"
        : currentAcpSession?.state === "active"
          ? "Live"
          : currentAcpSession
            ? `${currentAcpSession.state[0].toUpperCase()}${currentAcpSession.state.slice(1)}`
            : "Ready";
  const visibleHarnesses = harnesses.filter(
    (harness) =>
      harness.installState === "available" ||
      harness.source === "builtIn" ||
      harness.id === selectedHarnessId,
  );
  const selectedMatchesCurrentSession = Boolean(
    currentAcpSession &&
    currentAcpSession.bindingId === selectedAcpBindingId &&
    currentAcpSession.modelId === selectedModelId,
  );
  const actionBusy = useNativeAcp ? acpActions.busy : sending;
  const activeAcpTurn = useNativeAcp ? currentNonterminalTurn(acpPage) : null;
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
        (!selectedMatchesCurrentSession ||
          currentAcpSession?.capabilities.prompt),
      )
    : harnessAvailable;
  const composerEnabled = canPrompt && !activeAcpTurn;

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [messages.length]);

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
    void (target === "file" ? openFile(resource) : openDiff(resource));
  }

  return (
    <section ref={surfaceRef} className="chat-surface" aria-label="Agent chat">
      <header className="surface-header">
        <div>
          <p className="eyebrow">Persistent agent session</p>
          <h1>Sage workbench</h1>
        </div>
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

      <div className="message-list">
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
                onOpenResource={openAcpResource}
                hasEarlier={acpHistory.hasEarlier}
                loadingEarlier={acpHistory.loadingEarlier}
                historyError={acpHistory.historyError}
                onLoadEarlier={acpHistory.loadEarlier}
              />
            ) : (
              <>
                {selectedHarness?.source === "development" ? (
                  <div className="project-intro">
                    <span className="project-intro__icon">
                      <Check size={17} aria-hidden="true" />
                    </span>
                    <div>
                      <strong>Development simulator</strong>
                      <p>
                        This test-only harness exercises the workbench without
                        connecting a production agent.
                      </p>
                    </div>
                  </div>
                ) : null}
                {messages.map((message) => (
                  <article
                    className={`message message--${message.role}`}
                    key={message.id}
                  >
                    <div className="message__role">
                      {message.role === "user" ? "You" : "Sage"}
                    </div>
                    <p>{message.text}</p>
                  </article>
                ))}
              </>
            )}
            <EditSummary />
          </>
        )}
        <div ref={endRef} />
      </div>

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
        {!canPrompt ? (
          <div className="composer__notice">
            No reviewed local binding is available for this harness.
          </div>
        ) : null}
        <label className="sr-only" htmlFor="prompt">
          Message agent
        </label>
        <span className="sr-only" id="prompt-description">
          Send a message to the selected harness and model
        </span>
        <textarea
          ref={composerRef}
          id="prompt"
          aria-describedby="prompt-description"
          value={composerText}
          maxLength={MAX_CHAT_COMPOSER_BYTES}
          rows={3}
          placeholder={
            canPrompt
              ? "Ask Sage to do anything"
              : "Choose an available harness before starting a session"
          }
          disabled={!composerEnabled || actionBusy || projectTransitioning}
          onChange={(event) => setComposerText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && composerEnabled) {
              event.preventDefault();
              event.currentTarget.form?.requestSubmit();
            }
          }}
        />
        <div className="composer__toolbar">
          <div className="composer__controls">
            <label className="select-control">
              <span className="sr-only">Harness</span>
              <select
                value={selectedHarnessId ?? ""}
                disabled={projectTransitioning}
                onChange={(event) => void selectHarness(event.target.value)}
              >
                {visibleHarnesses.map((harness) => (
                  <option
                    key={harness.id}
                    value={harness.id}
                    disabled={harness.installState !== "available"}
                  >
                    {harness.name}
                    {harness.source === "development"
                      ? " — test only"
                      : harness.installState !== "available"
                        ? " — unavailable"
                        : ""}
                  </option>
                ))}
              </select>
              <ChevronDown size={14} aria-hidden="true" />
            </label>
            <label className="select-control">
              <span className="sr-only">Model</span>
              <select
                value={selectedModelId ?? ""}
                disabled={
                  projectTransitioning || !canPrompt || models.length === 0
                }
                onChange={(event) => selectModel(event.target.value)}
              >
                {models.length === 0 ? (
                  <option value="">Harness default</option>
                ) : (
                  models.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.name}
                    </option>
                  ))
                )}
              </select>
              <ChevronDown size={14} aria-hidden="true" />
            </label>
            {availableBinding ? (
              <span
                className="access-mode"
                title="Permission requests fail closed"
              >
                <ShieldCheck size={14} aria-hidden="true" />
                Read-only policy
              </span>
            ) : null}
          </div>
          {useNativeAcp && acpActions.cancellableTurnId ? (
            <button
              className="send-button"
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
      </form>
    </section>
  );
}
