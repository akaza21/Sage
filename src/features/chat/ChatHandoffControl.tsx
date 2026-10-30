import { ArrowRightLeft } from "../../ui/leanIcons";
import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import {
  buildChatHandoffDraft,
  type ChatHandoffContextEntry,
  type ChatHandoffIntent,
} from "../../domain/chatHandoff";
import { chatDisplayTitle } from "../../domain/chatTitle";
import type { ChatMessage } from "../../domain/workbench";
import { HandoffPart } from "./lazyParts";

interface ChatHandoffControlProps {
  acpPage: AcpChatPage | null;
  messages: readonly ChatMessage[];
  busy: boolean;
  onCommitted: () => void;
  menuItem?: boolean;
  onOpen?: () => void;
  returnFocusRef?: RefObject<HTMLButtonElement | null>;
  onReturnFocus?: () => void;
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

/**
 * The Hand off trigger and its intent. The dialog itself loads in its own
 * lazy chunk (ChatHandoffDialog.tsx) on the first open.
 */
export function ChatHandoffControl({
  acpPage,
  messages,
  busy,
  onCommitted,
  menuItem = false,
  onOpen,
  returnFocusRef,
  onReturnFocus,
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
  const [intent, setIntent] = useState<ChatHandoffIntent | null>(null);
  const [handoffError, setHandoffError] = useState<string | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const sourceTab = tabs.find(
    (tab) => tab.id === activeTabId && tab.kind === "chat",
  );
  const targets = useMemo(
    () => tabs.filter((tab) => tab.kind === "chat" && tab.id !== sourceTab?.id),
    [sourceTab?.id, tabs],
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
  const restoreFocus = () =>
    queueMicrotask(() =>
      onReturnFocus
        ? onReturnFocus()
        : (returnFocusRef?.current ?? triggerRef.current)?.focus(),
    );
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
        setHandoffError(null);
        queueMicrotask(() =>
          onReturnFocus
            ? onReturnFocus()
            : (returnFocusRef?.current ?? triggerRef.current)?.focus(),
        );
      }
    });
  }, [intent, returnFocusRef, onReturnFocus]);

  const close = () => {
    setIntent(null);
    setHandoffError(null);
    restoreFocus();
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
      onOpen?.();
    } catch (error) {
      setHandoffError(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <div className="handoff-control">
      <button
        ref={triggerRef}
        className={menuItem ? "menu-item" : "button button--compact"}
        type="button"
        aria-label="Hand off chat"
        role={menuItem ? "menuitem" : undefined}
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
        <HandoffPart
          part="ChatHandoffDialog"
          intent={currentIntent}
          targets={targets}
          error={handoffError}
          onIntentChange={setIntent}
          onError={setHandoffError}
          onClose={close}
          onCommitted={onCommitted}
          restoreFocus={restoreFocus}
        />
      ) : null}
    </div>
  );
}
