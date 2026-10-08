import { MoreHorizontal, X } from "../../ui/leanIcons";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";
import { QueuedAttachmentPreview } from "./ComposerAttachments";

export function ComposerQueuedPrompts({
  prompts,
  steerAvailable,
  disabled,
  onDelete,
  onRetry,
  onSteer,
}: {
  prompts?: readonly QueuedChatPrompt[];
  steerAvailable?: boolean;
  disabled: boolean;
  onDelete?: (id: string) => void;
  onRetry?: (id: string) => void;
  onSteer?: (id: string) => void;
}) {
  if (!prompts?.length) return null;
  return (
    <div className="composer__queued" aria-label="Queued messages">
      {prompts.map((prompt) => (
        <div className="composer__queued-row" key={prompt.id}>
          <QueuedAttachmentPreview contexts={prompt.attachedContext} />
          <span className="composer__queued-text" title={prompt.text}>
            {prompt.text}
          </span>
          <span className="composer__queued-state">
            {prompt.status === "saving"
              ? "Saving…"
              : prompt.status === "deleting"
                ? "Removing…"
                : prompt.status === "blocked"
                  ? "Needs retry"
                  : prompt.status === "outcomeUnknown"
                    ? "May have been delivered"
                    : prompt.status === "sending"
                      ? "Sending…"
                      : "Queued"}
          </span>
          {steerAvailable &&
          !["sending", "saving", "deleting"].includes(prompt.status) ? (
            <button
              type="button"
              className="composer__queued-steer"
              aria-label={`Steer queued message: ${prompt.text}`}
              title={
                prompt.status === "outcomeUnknown"
                  ? "Retry Steer with the same message ID"
                  : "Submit without interrupting the model"
              }
              disabled={disabled}
              onClick={() => onSteer?.(prompt.id)}
            >
              Steer
            </button>
          ) : null}
          <button
            type="button"
            aria-label={`Delete queued message: ${prompt.text}`}
            disabled={
              prompt.status === "saving" || prompt.status === "deleting"
            }
            onClick={() => onDelete?.(prompt.id)}
          >
            <X size={16} aria-hidden="true" />
          </button>
          <details className="composer__queued-options">
            <summary
              aria-label={`More options for queued message: ${prompt.text}`}
            >
              <MoreHorizontal size={16} aria-hidden="true" />
            </summary>
            <button
              type="button"
              onClick={() =>
                prompt.status === "blocked"
                  ? onRetry?.(prompt.id)
                  : onDelete?.(prompt.id)
              }
            >
              {prompt.status === "blocked" ? "Retry when ready" : "Delete"}
            </button>
          </details>
        </div>
      ))}
    </div>
  );
}
