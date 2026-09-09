import { ArrowRightLeft } from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
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
import { ModalDialog } from "../../ui/ModalDialog";
import { SearchablePicker } from "../../ui/SearchablePicker";

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
  const sourceTab = tabs.find(
    (tab) => tab.id === activeTabId && tab.kind === "chat",
  );
  const targets = useMemo(
    () => tabs.filter((tab) => tab.kind === "chat" && tab.id !== sourceTab?.id),
    [sourceTab?.id, tabs],
  );
  const targetCandidates = useMemo(
    () =>
      targets.map((target) => ({
        id: target.id,
        label: chatDisplayTitle(target.title),
      })),
    [targets],
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
        queueMicrotask(() => triggerRef.current?.focus());
      }
    });
  }, [intent]);

  const close = () => {
    setIntent(null);
    setSaving(false);
    setHandoffError(null);
    queueMicrotask(() => triggerRef.current?.focus());
  };
  const chooseTarget = useCallback(
    (targetChatId: string) => {
      if (!currentIntent) return;
      setIntent({ ...currentIntent, targetChatId });
    },
    [currentIntent],
  );

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
        <ModalDialog
          className="handoff-dialog"
          labelledBy="handoff-dialog-title"
          onDismiss={() => {
            if (!saving) close();
          }}
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
              <SearchablePicker
                ariaLabel="Target Chat"
                candidates={targetCandidates}
                disabled={saving || projectTransitioning}
                onSelect={chooseTarget}
                portalTarget="closest-dialog"
                selectedId={currentIntent.targetChatId}
              />
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
        </ModalDialog>
      ) : null}
    </div>
  );
}
