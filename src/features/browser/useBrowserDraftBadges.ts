import { useEffect, useRef } from "react";
import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import {
  isBrowserElement,
  type BrowserElementContext,
} from "../../domain/attachedContext";
import { invokeNative } from "../../ports/nativeInvoke";

type Update = { tabId: string; annotations: BrowserElementContext[] };

/** One in-flight update and at most eight replaceable tab snapshots. */
export function useBrowserDraftBadges(
  tabId: string | undefined,
  url: string | undefined,
) {
  const owner = usePinnedAgentChatOwner();
  const contexts = useWorkbenchStore((state) =>
    owner ? state.chatUiById[owner]?.attachedContext : undefined,
  );
  const pending = useRef(new Map<string, Update>());
  const running = useRef(false);
  const send = (update: Update) => {
    if (!pending.current.has(update.tabId) && pending.current.size === 8)
      pending.current.delete(pending.current.keys().next().value!);
    pending.current.set(update.tabId, update);
    if (running.current) return;
    running.current = true;
    void (async () => {
      try {
        while (pending.current.size) {
          const next = pending.current.values().next().value!;
          pending.current.delete(next.tabId);
          await invokeNative("browser_control", {
            ...next,
            action: "annotateBadges",
          }).catch(() => undefined);
        }
      } finally {
        running.current = false;
      }
    })();
  };
  useEffect(() => {
    if (!tabId) return;
    send({
      tabId,
      annotations: (contexts ?? [])
        .filter(isBrowserElement)
        .filter((item) => item.tabId === tabId)
        .slice(0, 8),
    });
    return () => send({ tabId, annotations: [] });
  }, [tabId, url, contexts]);
}
