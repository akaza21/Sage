import { useEffect, useRef, type FormEvent } from "react";
import { AlertTriangle, Check, CircleAlert, Info, X } from "../ui/leanIcons";
import { MAX_CHAT_TITLE_BYTES } from "../domain/chatTitle";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";

export function AgentTaskStatus({
  snapshot,
  harnessStatus,
}: {
  snapshot: ChatLiveSnapshot | null;
  harnessStatus?: string;
}) {
  const label = snapshot?.statusLabel ?? "Idle";
  const tone = snapshot?.statusTone ?? "idle";
  const StatusIcon =
    tone === "waiting" ? AlertTriangle : tone === "failed" ? CircleAlert : null;
  return (
    <>
      <span
        className={`agent-task-header__status agent-chat-chip agent-chat-chip--${tone}`}
        role="status"
        aria-label={`Chat status: ${label}`}
        title={label}
        tabIndex={0}
      >
        {tone === "working" ? (
          <span className="agent-chat-chip__spinner" aria-hidden="true" />
        ) : null}
        {StatusIcon ? (
          <StatusIcon
            className="agent-task-header__status-icon"
            size={12}
            aria-hidden="true"
          />
        ) : null}
        <span className="agent-task-header__status-label">{label}</span>
      </span>
      {harnessStatus ? (
        <details className="agent-task-header__harness-status">
          <summary
            aria-label={`Harness status: ${harnessStatus}`}
            title={harnessStatus}
            tabIndex={0}
          >
            <Info
              className="agent-task-header__harness-icon"
              size={14}
              aria-hidden="true"
            />
            <span>{harnessStatus}</span>
          </summary>
          <p>{harnessStatus}</p>
        </details>
      ) : null}
    </>
  );
}

// The header's inline rename, opened by its pencil or the chat actions.
export function AgentTaskRenameForm({
  value,
  error,
  disabled,
  onChange,
  onSubmit,
  onCancel,
}: {
  value: string;
  error: string | null;
  disabled: boolean;
  onChange: (value: string) => void;
  onSubmit: (event: FormEvent) => void;
  onCancel: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
  }, []);
  return (
    <form className="agent-task-header__rename" onSubmit={onSubmit}>
      <input
        ref={input}
        aria-label="New chat name"
        aria-invalid={error ? true : undefined}
        maxLength={MAX_CHAT_TITLE_BYTES}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
        }}
      />
      <button type="submit" aria-label="Save chat name" disabled={disabled}>
        <Check size={14} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Cancel chat rename"
        disabled={disabled}
        onClick={onCancel}
      >
        <X size={14} aria-hidden="true" />
      </button>
      {error ? <span role="alert">{error}</span> : null}
    </form>
  );
}
