import { ArrowUp, Check, ChevronDown, Plus, ShieldCheck } from "lucide-react";
import { type FormEvent, useEffect, useRef } from "react";

import { useWorkbenchStore } from "../../app/store";
import { EditSummary } from "./EditSummary";

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
  const error = useWorkbenchStore((state) => state.error);
  const selectHarness = useWorkbenchStore((state) => state.selectHarness);
  const selectModel = useWorkbenchStore((state) => state.selectModel);
  const setComposerText = useWorkbenchStore((state) => state.setComposerText);
  const sendPrompt = useWorkbenchStore((state) => state.sendPrompt);
  const endRef = useRef<HTMLDivElement>(null);

  const selectedHarness = harnesses.find(
    (harness) => harness.id === selectedHarnessId,
  );
  const harnessAvailable = selectedHarness?.installState === "available";

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [messages.length]);

  function submit(event: FormEvent) {
    event.preventDefault();
    void sendPrompt();
  }

  return (
    <section className="chat-surface" aria-label="Agent chat">
      <header className="surface-header">
        <div>
          <p className="eyebrow">Persistent agent session</p>
          <h1>Walking skeleton</h1>
        </div>
        <div className="session-state">
          <span className="session-state__dot" />
          Live
        </div>
      </header>

      <div className="message-list" aria-live="polite">
        <div className="project-intro">
          <span className="project-intro__icon">
            <Check size={17} aria-hidden="true" />
          </span>
          <div>
            <strong>Phase 0 has started</strong>
            <p>
              This first vertical slice proves the tab-first workbench,
              harness-then-model selection, and Rust-owned durable transcript.
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
        <EditSummary />
        <div ref={endRef} />
      </div>

      <form className="composer" onSubmit={submit}>
        {error ? <div className="composer__error">{error}</div> : null}
        {!harnessAvailable ? (
          <div className="composer__notice">
            This curated harness is not installed. Managed installation will be
            added in the ACP runtime phase.
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
            harnessAvailable
              ? "Ask Sage to do anything"
              : "Install this harness before starting a session"
          }
          disabled={!harnessAvailable || sending}
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
          >
            <Plus size={18} aria-hidden="true" />
          </button>
          <div className="composer__controls">
            <label className="select-control">
              <span className="sr-only">Harness</span>
              <select
                value={selectedHarnessId ?? ""}
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
                disabled={!harnessAvailable || models.length === 0}
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
            <span className="access-mode" title="Project-scoped access">
              <ShieldCheck size={14} aria-hidden="true" />
              Project access
            </span>
          </div>
          <button
            className="send-button"
            type="submit"
            aria-label="Send prompt"
            disabled={!composerText.trim() || !harnessAvailable || sending}
          >
            <ArrowUp size={18} aria-hidden="true" />
          </button>
        </div>
      </form>
    </section>
  );
}
