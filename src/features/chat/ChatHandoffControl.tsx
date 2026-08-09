import { ArrowRightLeft } from "lucide-react";
import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import {
  MAX_HANDOFF_DRAFT_BYTES,
  buildChatHandoffDraft,
  type ChatHandoffContextEntry,
  type ChatHandoffIntent,
} from "../../domain/chatHandoff";
import { chatDisplayTitle } from "../../domain/chatTitle";
import type { ChatMessage } from "../../domain/workbench";

interface ChatHandoffControlProps {
  acpPage: AcpChatPage | null;
  messages: readonly ChatMessage[];
  busy: boolean;
  onCommitted: () => void;
}

function sourceContext(
  acpPage: AcpChatPage | null,
  messages: readonly ChatMessage[],
): ChatHandoffContextEntry[] {
  if (!acpPage) {
    return messages.map(({ role, text }) => ({ role, text }));
  }
  return acpPage.turns.flatMap((turn) =>
    turn.items.flatMap((item) =>
      item.kind === "text" && item.origin === "primary"
        ? [{ role: item.payload.role, text: item.payload.text }]
        : [],
    ),
  );
}

export function ChatHandoffControl({
  acpPage,
  messages,
  busy,
  onCommitted,
}: ChatHandoffControlProps) {
  const project = useWorkbenchStore((state) => state.project);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const handoffChat = useWorkbenchStore((state) => state.handoffChat);
  const [intent, setIntent] = useState<ChatHandoffIntent | null>(null);
  const [saving, setSaving] = useState(false);
  const [handoffError, setHandoffError] = useState<string | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const dialogRef = useRef<HTMLDialogElement | null>(null);
  const sourceTab = tabs.find(
    (tab) => tab.id === activeTabId && tab.kind === "chat",
  );
  const targets = tabs.filter(
    (tab) => tab.kind === "chat" && tab.id !== sourceTab?.id,
  );
  const disabled = busy || projectTransitioning || !sourceTab || !project;
  const currentIntent =
    intent &&
    !projectTransitioning &&
    project?.id === intent.projectId &&
    projectGeneration === intent.projectGeneration &&
    transitionEpoch === intent.transitionEpoch &&
    activeTabId === intent.sourceChatId
      ? intent
      : null;
  const dialogOpen = Boolean(currentIntent);

  useEffect(() => {
    if (!intent) return;
    return useWorkbenchStore.subscribe((state) => {
      if (
        state.projectTransitioning ||
        state.project?.id !== intent.projectId ||
        state.projectGeneration !== intent.projectGeneration ||
        state.nativeTransitionEpoch !== intent.transitionEpoch ||
        state.activeTabId !== intent.sourceChatId
      ) {
        setIntent(null);
        setSaving(false);
        setHandoffError(null);
      }
    });
  }, [intent]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialogOpen || !dialog) return;
    if (typeof dialog.showModal === "function") {
      if (!dialog.open) dialog.showModal();
    } else {
      dialog.setAttribute("open", "");
    }
    return () => {
      if (typeof dialog.close === "function" && dialog.open) {
        dialog.close();
      } else {
        dialog.removeAttribute("open");
      }
    };
  }, [dialogOpen]);

  const close = () => {
    setIntent(null);
    setSaving(false);
    setHandoffError(null);
    queueMicrotask(() => triggerRef.current?.focus());
  };

  const open = () => {
    if (disabled || !sourceTab || !project || targets.length === 0) return;
    try {
      setIntent({
        projectId: project.id,
        projectGeneration,
        transitionEpoch,
        sourceChatId: sourceTab.id,
        targetChatId: targets[0].id,
        draft: buildChatHandoffDraft(
          chatDisplayTitle(sourceTab.title),
          sourceContext(acpPage, messages),
        ),
      });
      setHandoffError(null);
    } catch (error) {
      setHandoffError(error instanceof Error ? error.message : String(error));
    }
  };

  const confirm = async (event: FormEvent) => {
    event.preventDefault();
    if (!currentIntent || saving) return;
    setSaving(true);
    setHandoffError(null);
    try {
      const committed = await handoffChat(currentIntent);
      setSaving(false);
      setIntent(null);
      if (committed) {
        onCommitted();
        return;
      }
      setHandoffError(
        useWorkbenchStore.getState().error ??
          "Chat Handoff could not be placed",
      );
    } catch (error) {
      setSaving(false);
      setIntent(null);
      setHandoffError(error instanceof Error ? error.message : String(error));
    }
    queueMicrotask(() => triggerRef.current?.focus());
  };

  const containFocus = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (event.key === "Escape" && !saving) {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>(
        "button:not([disabled]), select:not([disabled]), textarea:not([disabled])",
      ),
    );
    const first = focusable[0];
    const last = focusable.at(-1);
    if (!first || !last) {
      event.preventDefault();
      return;
    }
    if (
      event.shiftKey &&
      (document.activeElement === first ||
        !event.currentTarget.contains(document.activeElement))
    ) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div className="handoff-control">
      <button
        ref={triggerRef}
        className="button button--compact"
        type="button"
        aria-label="Hand off chat"
        disabled={disabled || targets.length === 0}
        onClick={open}
      >
        <ArrowRightLeft size={14} aria-hidden="true" />
        Hand off
      </button>
      {handoffError && !currentIntent ? (
        <div className="handoff-control__error" role="alert">
          {handoffError}
        </div>
      ) : null}
      {currentIntent ? (
        <dialog
          ref={dialogRef}
          className="dirty-dialog handoff-dialog"
          aria-modal="true"
          aria-labelledby="handoff-dialog-title"
          onCancel={(event) => {
            event.preventDefault();
            if (!saving) close();
          }}
          onKeyDown={containFocus}
        >
          <form
            className="handoff-dialog__form"
            aria-labelledby="handoff-dialog-title"
            onSubmit={(event) => void confirm(event)}
          >
            <h2 id="handoff-dialog-title">Hand off this Chat</h2>
            <p>
              Review the bounded context as an ordinary prompt. It will be
              placed in the target composer and will not be sent automatically.
            </p>
            <label className="handoff-dialog__field">
              <span>Target Chat</span>
              <select
                aria-label="Target Chat"
                disabled={saving || projectTransitioning}
                value={currentIntent.targetChatId}
                onChange={(event) =>
                  setIntent({
                    ...currentIntent,
                    targetChatId: event.target.value,
                  })
                }
              >
                {targets.map((target) => (
                  <option key={target.id} value={target.id}>
                    {chatDisplayTitle(target.title)}
                  </option>
                ))}
              </select>
            </label>
            <label className="handoff-dialog__field">
              <span>Handoff prompt</span>
              <textarea
                aria-label="Handoff prompt"
                autoFocus
                disabled={saving || projectTransitioning}
                maxLength={MAX_HANDOFF_DRAFT_BYTES}
                rows={10}
                value={currentIntent.draft}
                onChange={(event) =>
                  setIntent({ ...currentIntent, draft: event.target.value })
                }
              />
            </label>
            {handoffError ? (
              <div className="composer__error" role="alert">
                {handoffError}
              </div>
            ) : null}
            <div className="dirty-dialog__actions">
              <button
                className="button"
                type="button"
                disabled={saving || projectTransitioning}
                onClick={close}
              >
                Cancel
              </button>
              <button
                className="button button--primary"
                type="submit"
                disabled={
                  saving || projectTransitioning || !currentIntent.draft.trim()
                }
              >
                Place in target
              </button>
            </div>
          </form>
        </dialog>
      ) : null}
    </div>
  );
}
