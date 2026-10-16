import { MoreHorizontal, X } from "../../ui/leanIcons";
import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { MenuPopover } from "../../ui/MenuPopover";
import type { QueuedChatPrompt } from "../../domain/chatOwnership";
import { QueuedAttachmentPreview } from "./ComposerAttachments";

export function ComposerQueuedPrompts({
  composerRef,
  prompts,
  steerAvailable,
  disabled,
  onDelete,
  onRetry,
  onSteer,
}: {
  composerRef?: RefObject<HTMLTextAreaElement | null>;
  prompts?: readonly QueuedChatPrompt[];
  steerAvailable?: boolean;
  disabled: boolean;
  onDelete?: (id: string) => void;
  onRetry?: (id: string) => void;
  onSteer?: (id: string) => void;
}) {
  const [menuId, setMenuId] = useState<string | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const rowTriggersRef = useRef(new Map<string, HTMLButtonElement>());
  const panelId = useId();
  const queueRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const queue = queueRef.current;
    const composer = queue?.closest<HTMLElement>(".composer");
    if (!queue || !composer) return;
    let frame = 0;
    const measure = () => {
      const space =
        getComputedStyle(queue).position === "absolute"
          ? composer.getBoundingClientRect().top -
            queue.getBoundingClientRect().top
          : 0;
      const value = space + "px";
      if (composer.style.getPropertyValue("--queued-tray-space") !== value)
        composer.style.setProperty("--queued-tray-space", value);
    };
    measure();
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(() => {
            if (frame) return;
            frame = requestAnimationFrame(() => {
              frame = 0;
              measure();
            });
          });
    observer?.observe(queue);
    return () => {
      observer?.disconnect();
      cancelAnimationFrame(frame);
      composer.style.removeProperty("--queued-tray-space");
    };
  }, [prompts?.length]);
  if (disabled && menuId !== null) setMenuId(null);
  const selected = prompts?.find((prompt) => prompt.id === menuId);
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
    setMenuId(null);
    (nextTrigger ?? composerRef?.current)?.focus();
  };
  return (
    <div
      ref={queueRef}
      className="composer__queued"
      aria-label="Queued messages"
    >
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
            onClick={() => deletePrompt(prompt.id)}
          >
            <X size={16} aria-hidden="true" />
          </button>
          <button
            ref={(trigger) => {
              if (trigger) rowTriggersRef.current.set(prompt.id, trigger);
              else rowTriggersRef.current.delete(prompt.id);
            }}
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
            if (selected.status === "blocked") {
              onRetry?.(selected.id);
              setMenuId(null);
              triggerRef.current?.focus();
            } else deletePrompt(selected.id);
          }}
        >
          {selected?.status === "blocked" ? "Retry when ready" : "Delete"}
        </button>
      </MenuPopover>
    </div>
  );
}
