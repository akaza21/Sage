import { useCallback, useEffect, useMemo } from "react";
import {
  changeQueuedChatPrompt,
  persistQueuedChatPrompt,
} from "../../app/chatAttachedContextState";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, ClientIntentId } from "../../domain/acp";
import type { AttachedContext } from "../../domain/attachedContext";
import type {
  ChatOwnershipEntry,
  ChatTurnOrder,
  QueuedChatPrompt,
} from "../../domain/chatOwnership";
const EMPTY_QUEUED_PROMPTS: readonly QueuedChatPrompt[] = [];

export function laterTurnOrder(
  left: ChatTurnOrder,
  right: ChatTurnOrder,
): boolean {
  return (
    left.sessionOrdinal > right.sessionOrdinal ||
    (left.sessionOrdinal === right.sessionOrdinal &&
      left.turnOrdinal > right.turnOrdinal)
  );
}

function latestTurnOrder(page: AcpChatPage | null): ChatTurnOrder | null {
  if (!page) return null;
  const sessions = new Map<string, number>();
  for (const session of page.sessions)
    sessions.set(session.id, session.ordinal);
  let latest: ChatTurnOrder | null = null;
  for (const turn of page.turns) {
    const sessionOrdinal = sessions.get(turn.sessionId);
    if (sessionOrdinal === undefined) continue;
    const order = { sessionOrdinal, turnOrdinal: turn.ordinal };
    if (!latest || laterTurnOrder(order, latest)) latest = order;
  }
  return latest;
}

export function useChatQueueState(
  chatId?: string,
  page: AcpChatPage | null = null,
) {
  const queuedPrompts = useWorkbenchStore((state) => {
    const ownerId = chatId ?? state.activeTabId;
    return ownerId
      ? (state.chatUiById[ownerId]?.queuedPrompts ?? EMPTY_QUEUED_PROMPTS)
      : EMPTY_QUEUED_PROMPTS;
  });
  function queuePrompt(bindingId: string, modelId: string | null) {
    return persistQueuedChatPrompt(
      useWorkbenchStore,
      chatId,
      bindingId,
      modelId,
    );
  }

  function updateQueuedPrompt(
    id: string,
    status: QueuedChatPrompt["status"] | "deleted",
  ) {
    changeQueuedChatPrompt(useWorkbenchStore, chatId, id, status);
  }

  function steerQueuedPrompt(
    id: string,
    steer: (
      text: string,
      context: readonly AttachedContext[],
      queuedMessageId: ClientIntentId,
    ) => Promise<unknown>,
  ) {
    const queued = queuedPrompts.find((prompt) => prompt.id === id);
    if (!queued || queued.status === "sending") return;
    updateQueuedPrompt(id, "sending");
    void steer(queued.text, queued.attachedContext, queued.id as ClientIntentId)
      .then((result) =>
        updateQueuedPrompt(
          id,
          result === "applied"
            ? "deleted"
            : result === "outcomeUnknown"
              ? "outcomeUnknown"
              : "blocked",
        ),
      )
      .catch(() => updateQueuedPrompt(id, "blocked"));
  }

  const changeQueueState = useCallback(
    (
      change: Partial<
        Pick<ChatOwnershipEntry, "queuedTurnBarrier" | "knownTurnOrder">
      >,
    ) => {
      const state = useWorkbenchStore.getState();
      const ownerId = chatId ?? state.activeTabId;
      const intent = ownerId ? state.captureChatOwnership(ownerId) : null;
      if (!intent) return;
      useWorkbenchStore.setState((current) => {
        if (
          current.project?.id !== intent.projectId ||
          current.chatOwnershipEpoch !== intent.ownershipEpoch
        )
          return current;
        const owned = current.chatUiById[intent.chatId];
        if (!owned) return current;
        if (
          change.knownTurnOrder &&
          owned.knownTurnOrder &&
          !laterTurnOrder(change.knownTurnOrder, owned.knownTurnOrder)
        )
          return current;
        return {
          chatUiById: {
            ...current.chatUiById,
            [intent.chatId]: { ...owned, ...change },
          },
        };
      });
    },
    [chatId],
  );

  function setQueuedTurnBarrier(
    barrier: { afterOrder: ChatTurnOrder | null; deadlineMs: number } | null,
  ) {
    changeQueueState({ queuedTurnBarrier: barrier });
  }

  const observedOrder = useMemo(() => latestTurnOrder(page), [page]);
  useEffect(() => {
    if (observedOrder) changeQueueState({ knownTurnOrder: observedOrder });
  }, [changeQueueState, observedOrder]);

  return {
    queuedPrompts,
    observedOrder,
    queuePrompt,
    updateQueuedPrompt,
    steerQueuedPrompt,
    setQueuedTurnBarrier,
  };
}
