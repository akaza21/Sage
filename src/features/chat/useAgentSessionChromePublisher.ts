import { useEffect, useRef, type RefObject } from "react";

import type { AgentSessionChrome } from "../../app/agentChatOwner";

type Published = Omit<AgentSessionChrome, "chatId" | "actions">;

/**
 * Publishes the focused chat's session chrome to the Agent header and keeps
 * the header's session actions pointed at the latest callbacks.
 */
export function useAgentSessionChromePublisher(
  publish: ((chrome: AgentSessionChrome | null) => void) | null,
  chatId: string | undefined,
  focused: boolean,
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
  const { harnessStatus, canCloseSession, canEndSession, busy } = chrome;
  const { acpPage, messages } = chrome;
  useEffect(() => {
    if (!publish || !chatId || !focused) return;
    publish({
      chatId,
      harnessStatus,
      canCloseSession,
      canEndSession,
      busy,
      acpPage,
      messages,
      actions: sessionActions,
    });
    return () => publish(null);
  }, [
    publish,
    chatId,
    focused,
    harnessStatus,
    canCloseSession,
    canEndSession,
    busy,
    acpPage,
    messages,
  ]);
}
