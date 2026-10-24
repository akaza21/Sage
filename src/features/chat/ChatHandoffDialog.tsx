import { useCallback, useMemo, useState, type FormEvent } from "react";

import { useWorkbenchStore } from "../../app/store";
import {
  MAX_HANDOFF_DRAFT_BYTES,
  type ChatHandoffIntent,
} from "../../domain/chatHandoff";
import { chatDisplayTitle } from "../../domain/chatTitle";
import type { WorkbenchTab } from "../../domain/workbench";
import { ModalDialog } from "../../ui/ModalDialog";
import { SearchablePicker } from "../../ui/SearchablePicker";

/**
 * Lazy handoff chunk: the "Hand off this Chat" dialog. The eager
 * ChatHandoffControl owns the trigger, the intent and its staleness checks;
 * this mounts only while a current intent exists.
 */
export function ChatHandoffDialog({
  intent,
  targets,
  error,
  onIntentChange,
  onError,
  onClose,
  onCommitted,
  restoreFocus,
}: {
  intent: ChatHandoffIntent;
  targets: readonly WorkbenchTab[];
  error: string | null;
  onIntentChange: (intent: ChatHandoffIntent | null) => void;
  onError: (error: string | null) => void;
  onClose: () => void;
  onCommitted: () => void;
  restoreFocus: () => void;
}) {
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const handoffChat = useWorkbenchStore((state) => state.handoffChat);
  const [saving, setSaving] = useState(false);
  const targetCandidates = useMemo(
    () =>
      targets.map((target) => ({
        id: target.id,
        label: chatDisplayTitle(target.title),
      })),
    [targets],
  );
  const chooseTarget = useCallback(
    (targetChatId: string) => onIntentChange({ ...intent, targetChatId }),
    [intent, onIntentChange],
  );
  const confirm = async (event: FormEvent) => {
    event.preventDefault();
    if (saving) return;
    setSaving(true);
    onError(null);
    try {
      const committed = await handoffChat(intent);
      setSaving(false);
      onIntentChange(null);
      if (committed) {
        onCommitted();
        return;
      }
      onError(
        useWorkbenchStore.getState().error ??
          "Chat Handoff could not be placed",
      );
    } catch (caught) {
      setSaving(false);
      onIntentChange(null);
      onError(caught instanceof Error ? caught.message : String(caught));
    }
    restoreFocus();
  };
  const locked = saving || projectTransitioning;

  return (
    <ModalDialog
      className="handoff-dialog"
      labelledBy="handoff-dialog-title"
      onDismiss={() => {
        if (!saving) onClose();
      }}
    >
      <form
        className="handoff-dialog__form"
        aria-labelledby="handoff-dialog-title"
        onSubmit={(event) => void confirm(event)}
      >
        <h2 id="handoff-dialog-title">Hand off this Chat</h2>
        <p>
          Review the bounded context as an ordinary prompt. It will be placed in
          the target composer and will not be sent automatically.
        </p>
        <label className="handoff-dialog__field">
          <span>Target Chat</span>
          <SearchablePicker
            ariaLabel="Target Chat"
            candidates={targetCandidates}
            disabled={locked}
            onSelect={chooseTarget}
            portalTarget="closest-dialog"
            selectedId={intent.targetChatId}
          />
        </label>
        <label className="handoff-dialog__field">
          <span>Handoff prompt</span>
          <textarea
            aria-label="Handoff prompt"
            autoFocus
            disabled={locked}
            maxLength={MAX_HANDOFF_DRAFT_BYTES}
            rows={10}
            value={intent.draft}
            onChange={(event) =>
              onIntentChange({ ...intent, draft: event.target.value })
            }
          />
        </label>
        {error ? (
          <div className="composer__error" role="alert">
            {error}
          </div>
        ) : null}
        <div className="dirty-dialog__actions">
          <button
            className="button"
            type="button"
            disabled={locked}
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            className="button button--primary"
            type="submit"
            disabled={locked || !intent.draft.trim()}
          >
            Place in target
          </button>
        </div>
      </form>
    </ModalDialog>
  );
}
