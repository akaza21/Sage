import { useEffect, useRef, type RefObject } from "react";

import type { AgentSessionChrome } from "../../app/agentChatOwner";
import { useSideChatState } from "../../workbench/sideChatState";

type Published = Omit<AgentSessionChrome, "chatId" | "actions">;

// A parent whose side chat holds focus keeps publishing its own live chrome, so
// its header cell never mixes its status with another Chat's session.
export function useAgentSessionChromePublisher(
  publish: ((chatId: string, chrome: AgentSessionChrome | null) => void) | null,
  chatId: string | undefined,
  focusedChatId: string | null,
  chrome: Published,
  actions: { closeSession: () => void; endSession: () => void },
  composerRef: RefObject<HTMLTextAreaElement | null>,
) {
  const sessionActions = useRef({
    ...actions,
    focusComposer: () => composerRef.current?.focus(),
  });
  const { closeSession, endSession } = actions;
  useEffect(() => {
    Object.assign(sessionActions.current, { closeSession, endSession });
  }, [closeSession, endSession]);
  const heldBySide = useSideChatState((state) =>
    state.sides.some(
      (side) => side.parentChatId === chatId && side.id === focusedChatId,
    ),
  );
  const shown = focusedChatId === chatId || heldBySide;
  const { harnessStatus, canCloseSession, canEndSession, busy } = chrome;
  const { acpPage, messages } = chrome;
  useEffect(() => {
    if (!publish || !chatId || !shown) return;
    publish(chatId, {
      chatId,
      harnessStatus,
      canCloseSession,
      canEndSession,
      busy,
      acpPage,
      messages,
      actions: sessionActions,
    });
    return () => publish(chatId, null);
  }, [
    publish,
    chatId,
    shown,
    harnessStatus,
    canCloseSession,
    canEndSession,
    busy,
    acpPage,
    messages,
  ]);
}
