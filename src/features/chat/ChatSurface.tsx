import { ArrowUp, Check, ChevronDown, Plus, ShieldCheck } from "lucide-react";
import { type FormEvent, useEffect, useRef } from "react";

import { useWorkbenchStore } from "../../app/store";
import type {
  AcpChatPage,
  CuratedBindingId,
  CuratedModelId,
  TurnProjection,
} from "../../domain/acp";
import type { ResourceRef } from "../../domain/workbench";
import { EditSummary } from "./EditSummary";
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
          <div className="session-state">
            <span className="session-state__dot" />
            Live
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
                <div className="project-intro">
                  <span className="project-intro__icon">
                    <Check size={17} aria-hidden="true" />
                  </span>
                  <div>
                    <strong>Phase 0 has started</strong>
                    <p>
                      This first vertical slice proves the tab-first workbench,
                      harness-then-model selection, and Rust-owned durable
                      transcript.
                    </p>
                  </div>
                </div>
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
        {error || acpActions.error ? (
          <div className="composer__error" role="alert">
            {acpActions.error?.message ?? error}
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
          <button
            className="icon-button"
            type="button"
            aria-label="Attach context"
            title="Attach context"
            disabled
          >
            <Plus size={18} aria-hidden="true" />
          </button>
          <div className="composer__controls">
            <label className="select-control">
              <span className="sr-only">Harness</span>
              <select
                value={selectedHarnessId ?? ""}
                disabled={projectTransitioning}
                onChange={(event) => void selectHarness(event.target.value)}
              >
                {harnesses.map((harness) => (
                  <option key={harness.id} value={harness.id}>
                    {harness.name}
                    {harness.installState === "notInstalled"
                      ? " - install"
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
            <span
              className="access-mode"
              title="Permission requests fail closed"
            >
              <ShieldCheck size={14} aria-hidden="true" />
              Read-only policy
            </span>
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
