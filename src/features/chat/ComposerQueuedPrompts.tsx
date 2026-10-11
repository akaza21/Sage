import { MoreHorizontal, X } from "../../ui/leanIcons";
import { useId, useRef, useState } from "react";
import { MenuPopover } from "../../ui/MenuPopover";
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
  const [menuId, setMenuId] = useState<string | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const panelId = useId();
  if (disabled && menuId !== null) setMenuId(null);
  const selected = prompts?.find((prompt) => prompt.id === menuId);
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
          <button
            type="button"
            aria-label={`More options for queued message: ${prompt.text}`}
            aria-haspopup="menu"
            aria-controls={panelId}
            aria-expanded={menuId === prompt.id}
            disabled={
              disabled ||
              prompt.status === "saving" ||
              prompt.status === "deleting"
            }
            onClick={(event) => {
              triggerRef.current = event.currentTarget;
              setMenuId((current) =>
                current === prompt.id ? null : prompt.id,
              );
            }}
          >
            <MoreHorizontal size={16} aria-hidden="true" />
          </button>
        </div>
      ))}
      <MenuPopover
        id={panelId}
        ariaLabel="Queued message options"
        open={Boolean(selected) && !disabled}
        triggerRef={triggerRef}
        restoreFocusOnTab
        onDismiss={() => setMenuId(null)}
      >
        <button
          className="menu-item"
          type="button"
          role="menuitem"
          onClick={() => {
            if (!selected) return;
            if (selected.status === "blocked") onRetry?.(selected.id);
            else onDelete?.(selected.id);
            setMenuId(null);
            triggerRef.current?.focus();
          }}
        >
          {selected?.status === "blocked" ? "Retry when ready" : "Delete"}
        </button>
      </MenuPopover>
    </div>
  );
}
