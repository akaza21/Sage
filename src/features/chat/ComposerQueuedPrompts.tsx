import { useRef, useState, type RefObject } from "react";
import {
  ArrowUp,
  ChevronDown,
  Pencil,
  RotateCcw,
  Trash2,
} from "../../ui/leanIcons";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";
import { QueuedAttachmentPreview } from "./QueuedAttachmentPreview";

const STATE_LABEL: Partial<Record<QueuedChatPrompt["status"], string>> = {
  saving: "Saving…",
  deleting: "Removing…",
  blocked: "Needs retry",
  outcomeUnknown: "May have been delivered",
  sending: "Sending…",
};

/**
 * Queued messages as their own card above the composer: a header with the
 * count and when they send, then a row per message with Send now (Steer,
 * where the harness takes it), Edit and Delete. The card folds to its header.
 */
export function ComposerQueuedPrompts({
  composerRef,
  prompts,
  steerAvailable,
  awaitingApproval = false,
  disabled,
  onDelete,
  onRetry,
  onSteer,
  onEdit,
}: {
  composerRef?: RefObject<HTMLTextAreaElement | null>;
  prompts?: readonly QueuedChatPrompt[];
  steerAvailable?: boolean;
  awaitingApproval?: boolean;
  disabled: boolean;
  onDelete?: (id: string) => void;
  onRetry?: (id: string) => void;
  onSteer?: (id: string) => void;
  /** Puts a text-only message back in the composer. */
  onEdit?: (text: string) => void;
}) {
  const [folded, setFolded] = useState(false);
  const rowTriggersRef = useRef(new Map<string, HTMLButtonElement>());
  if (!prompts?.length) return null;
  const deletePrompt = (id: string) => {
    const index = prompts.findIndex((prompt) => prompt.id === id);
    const adjacent = [
      ...prompts.slice(index + 1),
      ...prompts.slice(0, index).reverse(),
    ];
    const nextTrigger = adjacent
      .map((prompt) => rowTriggersRef.current.get(prompt.id))
      .find((trigger) => trigger && !trigger.disabled);
    onDelete?.(id);
    (nextTrigger ?? composerRef?.current)?.focus();
  };
  return (
    <section className="composer__queued" aria-label="Queued messages">
      <header className="composer__queued-header">
        <b>Queued messages</b>
        <span className="composer__queued-count">{prompts.length}</span>
        <span className="composer__queued-when">
          {awaitingApproval
            ? "Waiting on your approval"
            : "Sends after the agent finishes"}
        </span>
        <button
          type="button"
          className="composer__queued-fold"
          aria-label={`${folded ? "Expand" : "Collapse"} queued messages`}
          aria-expanded={!folded}
          onClick={() => setFolded(!folded)}
        >
          <ChevronDown size={14} aria-hidden="true" />
        </button>
      </header>
      {folded
        ? null
        : prompts.map((prompt) => {
            const busy = ["sending", "saving", "deleting"].includes(
              prompt.status,
            );
            const textOnly = prompt.attachedContext.length === 0;
            return (
              <div className="composer__queued-row" key={prompt.id}>
                <QueuedAttachmentPreview contexts={prompt.attachedContext} />
                <span className="composer__queued-text" title={prompt.text}>
                  {prompt.text || "Attached context"}
                </span>
                {STATE_LABEL[prompt.status] ? (
                  <span className="composer__queued-state">
                    {STATE_LABEL[prompt.status]}
                  </span>
                ) : null}
                {prompt.status === "blocked" ? (
                  <button
                    type="button"
                    aria-label={`Retry queued message: ${prompt.text}`}
                    title="Retry when ready"
                    disabled={disabled}
                    onClick={() => onRetry?.(prompt.id)}
                  >
                    <RotateCcw size={14} aria-hidden="true" />
                  </button>
                ) : null}
                {steerAvailable &&
                // Steer is text-only, so a message with images waits its turn.
                !prompt.attachedContext.some(
                  (context) => context.kind === "image",
                ) &&
                !busy ? (
                  <button
                    type="button"
                    className="composer__queued-steer"
                    aria-label={`Send now: ${prompt.text}`}
                    title={
                      prompt.status === "outcomeUnknown"
                        ? "Retry with the same message ID"
                        : "Send now, without interrupting the agent"
                    }
                    disabled={disabled}
                    onClick={() => onSteer?.(prompt.id)}
                  >
                    <ArrowUp size={14} aria-hidden="true" />
                  </button>
                ) : null}
                {onEdit && textOnly && prompt.status !== "outcomeUnknown" ? (
                  <button
                    type="button"
                    aria-label={`Edit queued message: ${prompt.text}`}
                    title="Edit"
                    disabled={disabled || busy}
                    onClick={() => {
                      onEdit(prompt.text);
                      onDelete?.(prompt.id);
                      composerRef?.current?.focus();
                    }}
                  >
                    <Pencil size={14} aria-hidden="true" />
                  </button>
                ) : null}
                <button
                  ref={(trigger) => {
                    if (trigger) rowTriggersRef.current.set(prompt.id, trigger);
                    else rowTriggersRef.current.delete(prompt.id);
                  }}
                  type="button"
                  aria-label={`Delete queued message: ${prompt.text}`}
                  title="Delete"
                  disabled={
                    prompt.status === "saving" || prompt.status === "deleting"
                  }
                  onClick={() => deletePrompt(prompt.id)}
                >
                  <Trash2 size={14} aria-hidden="true" />
                </button>
              </div>
            );
          })}
    </section>
  );
}
