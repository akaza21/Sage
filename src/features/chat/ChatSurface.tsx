import { ArrowUp, Check, ChevronDown, Plus, ShieldCheck } from "lucide-react";
import { type FormEvent, useEffect, useRef } from "react";

import { useWorkbenchStore } from "../../app/store";
import type { CuratedBindingId, CuratedModelId } from "../../domain/acp";
import type { ResourceRef } from "../../domain/workbench";
import { EditSummary } from "./EditSummary";
import {
  AcpChatProjection,
  type OpenResourceTarget,
  useAcpChatActions,
  useAcpChatHistory,
  useAcpChatProjection,
} from "./acp";

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
  const endRef = useRef<HTMLDivElement>(null);

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
  const canPrompt = useNativeAcp
    ? Boolean(
        acpPage &&
        harnessAvailable &&
        selectedAcpBindingId &&
        (!selectedMatchesCurrentSession ||
          currentAcpSession?.capabilities.prompt),
      )
    : harnessAvailable;

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [messages.length]);

  function submit(event: FormEvent) {
    event.preventDefault();
    const bindingId = selectedAcpBindingId;
    if (useNativeAcp) {
      if (!bindingId) return;
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
    <section className="chat-surface" aria-label="Agent chat">
      <header className="surface-header">
        <div>
          <p className="eyebrow">Persistent agent session</p>
          <h1>Sage workbench</h1>
        </div>
        <div className="session-state">
          <span className="session-state__dot" />
          Live
        </div>
      </header>

      <div className="message-list" aria-live="polite">
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
                cancellableTurnId={acpActions.cancellableTurnId}
                onCancelTurn={(turnId) => void acpActions.cancel(turnId)}
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
          Prompt
        </label>
        <textarea
          id="prompt"
          value={composerText}
          rows={3}
          placeholder={
            canPrompt
              ? "Ask Sage to do anything"
              : "Choose an available harness before starting a session"
          }
          disabled={!canPrompt || actionBusy || projectTransitioning}
          onChange={(event) => setComposerText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
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
          <button
            className="send-button"
            type="submit"
            aria-label="Send prompt"
            disabled={
              projectTransitioning ||
              !composerText.trim() ||
              !canPrompt ||
              actionBusy
            }
          >
            <ArrowUp size={18} aria-hidden="true" />
          </button>
        </div>
      </form>
    </section>
  );
}
