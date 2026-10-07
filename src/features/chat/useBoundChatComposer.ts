import { useEffect, useMemo } from "react";
import { useWorkbenchStore } from "../../app/store";
import {
  changeQueuedChatPrompt,
  persistQueuedChatPrompt,
} from "../../app/chatAttachedContextState";
import {
  attachedContextKey,
  type AttachedContext,
} from "../../domain/attachedContext";
import type {
  AcpChatPage,
  ClientIntentId,
  CuratedBindingId,
  CuratedModelId,
} from "../../domain/acp";
import type {
  ChatTurnOrder,
  QueuedChatPrompt,
} from "../../domain/chatOwnership";

const EMPTY_ATTACHED_CONTEXT = [] as const;
const EMPTY_QUEUED_PROMPTS: readonly QueuedChatPrompt[] = [];
const QUEUED_TURN_START_TIMEOUT_MS = 30_000;

function laterTurnOrder(left: ChatTurnOrder, right: ChatTurnOrder): boolean {
  return (
    left.sessionOrdinal > right.sessionOrdinal ||
    (left.sessionOrdinal === right.sessionOrdinal &&
      left.turnOrdinal > right.turnOrdinal)
  );
}

function latestTurnOrder(page: AcpChatPage | null): ChatTurnOrder | null {
  if (!page) return null;
  const sessions = new Map(
    page.sessions.map((session) => [session.id, session.ordinal]),
  );
  let latest: ChatTurnOrder | null = null;
  for (const turn of page.turns) {
    const sessionOrdinal = sessions.get(turn.sessionId);
    if (sessionOrdinal === undefined) continue;
    const order = { sessionOrdinal, turnOrdinal: turn.ordinal };
    if (!latest || laterTurnOrder(order, latest)) latest = order;
  }
  return latest;
}

export function useBoundChatComposer(chatId?: string) {
  const composerText = useWorkbenchStore((state) => {
    if (!chatId) return state.composerText;
    return (
      state.chatUiById[chatId]?.composerText ??
      (state.activeTabId === chatId ? state.composerText : "")
    );
  });
  const attachedContext = useWorkbenchStore((state) => {
    const ownerId = chatId ?? state.activeTabId;
    return ownerId
      ? (state.chatUiById[ownerId]?.attachedContext ?? EMPTY_ATTACHED_CONTEXT)
      : EMPTY_ATTACHED_CONTEXT;
  });
  const queuedPrompts = useWorkbenchStore((state) => {
    const ownerId = chatId ?? state.activeTabId;
    return ownerId
      ? (state.chatUiById[ownerId]?.queuedPrompts ?? EMPTY_QUEUED_PROMPTS)
      : EMPTY_QUEUED_PROMPTS;
  });
  const queuedTurnBarrier = useWorkbenchStore((state) => {
    const ownerId = chatId ?? state.activeTabId;
    return ownerId
      ? (state.chatUiById[ownerId]?.queuedTurnBarrier ?? null)
      : null;
  });
  const knownTurnOrder = useWorkbenchStore((state) => {
    const ownerId = chatId ?? state.activeTabId;
    return ownerId ? (state.chatUiById[ownerId]?.knownTurnOrder ?? null) : null;
  });
  const sending = useWorkbenchStore((state) =>
    chatId ? (state.chatUiById[chatId]?.sending ?? false) : state.sending,
  );
  const chatError = useWorkbenchStore((state) =>
    chatId ? (state.chatUiById[chatId]?.error ?? null) : state.chatError,
  );
  const setComposerText = useWorkbenchStore((state) => state.setComposerText);
  const captureChatOwnership = useWorkbenchStore(
    (state) => state.captureChatOwnership,
  );
  const setChatComposer = useWorkbenchStore((state) => state.setChatComposer);
  const detachChatContext = useWorkbenchStore(
    (state) => state.detachChatContext,
  );
  const sendPrompt = useWorkbenchStore((state) => state.sendPrompt);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);

  function setBoundComposerText(text: string) {
    if (chatId) {
      const intent = captureChatOwnership(chatId);
      if (intent) {
        setChatComposer(intent, text);
        return;
      }
    }
    setComposerText(text);
  }

  function removeAttachedContext(contextKey: string) {
    const owner = chatId ?? activeTabId ?? undefined;
    const intent = owner ? captureChatOwnership(owner) : null;
    if (intent) detachChatContext(intent, contextKey);
  }

  async function sendBoundPrompt() {
    if (chatId && activeTabId !== chatId) {
      await activateTab(chatId);
    }
    await sendPrompt();
  }

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

  function setQueuedTurnBarrier(
    barrier: { afterOrder: ChatTurnOrder | null; deadlineMs: number } | null,
  ) {
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
      return {
        chatUiById: {
          ...current.chatUiById,
          [intent.chatId]: { ...owned, queuedTurnBarrier: barrier },
        },
      };
    });
  }

  function rememberTurnOrder(order: ChatTurnOrder | null) {
    if (!order) return;
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
      const existing = owned.knownTurnOrder;
      if (existing && !laterTurnOrder(order, existing)) return current;
      return {
        chatUiById: {
          ...current.chatUiById,
          [intent.chatId]: { ...owned, knownTurnOrder: order },
        },
      };
    });
  }

  return {
    composerText,
    attachedContext,
    queuedPrompts,
    queuedTurnBarrier,
    knownTurnOrder,
    sending,
    chatError,
    setComposerText: setBoundComposerText,
    removeAttachedContext,
    attachedContextKey,
    sendPrompt: sendBoundPrompt,
    queuePrompt,
    updateQueuedPrompt,
    steerQueuedPrompt,
    setQueuedTurnBarrier,
    rememberTurnOrder,
  };
}

export function useQueuedPromptDrain({
  queuedPrompts,
  queuedTurnBarrier,
  knownTurnOrder,
  activeTurn,
  actionBusy,
  projectTransitioning,
  canPrompt,
  useNativeAcp,
  page,
  submit,
  updateQueuedPrompt,
  setQueuedTurnBarrier,
  rememberTurnOrder,
}: {
  queuedPrompts: readonly QueuedChatPrompt[];
  queuedTurnBarrier: {
    afterOrder: ChatTurnOrder | null;
    deadlineMs: number;
  } | null;
  knownTurnOrder: ChatTurnOrder | null;
  activeTurn: boolean;
  actionBusy: boolean;
  projectTransitioning: boolean;
  canPrompt: boolean;
  useNativeAcp: boolean;
  page: AcpChatPage | null;
  submit: (
    text: string,
    context: readonly AttachedContext[],
    bindingId: CuratedBindingId,
    modelId: CuratedModelId | null,
  ) => Promise<unknown>;
  updateQueuedPrompt: (
    id: string,
    status: QueuedChatPrompt["status"] | "deleted",
  ) => void;
  setQueuedTurnBarrier: (
    barrier: { afterOrder: ChatTurnOrder | null; deadlineMs: number } | null,
  ) => void;
  rememberTurnOrder: (order: ChatTurnOrder | null) => void;
}) {
  const observedOrder = useMemo(() => latestTurnOrder(page), [page]);
  useEffect(() => {
    rememberTurnOrder(observedOrder);
  }, [rememberTurnOrder, observedOrder]);
  useEffect(() => {
    if (queuedTurnBarrier !== null) {
      if (
        !observedOrder ||
        (queuedTurnBarrier.afterOrder &&
          !laterTurnOrder(observedOrder, queuedTurnBarrier.afterOrder))
      ) {
        const remainingMs = queuedTurnBarrier.deadlineMs - Date.now();
        if (remainingMs <= 0) {
          const next = queuedPrompts[0];
          if (next?.status === "queued") updateQueuedPrompt(next.id, "blocked");
          setQueuedTurnBarrier(null);
          return;
        }
        const timer = setTimeout(() => {
          const next = queuedPrompts[0];
          if (next?.status === "queued") updateQueuedPrompt(next.id, "blocked");
          setQueuedTurnBarrier(null);
        }, remainingMs);
        return () => clearTimeout(timer);
      }
      setQueuedTurnBarrier(null);
      return;
    }
    const next = queuedPrompts[0];
    if (
      !next ||
      next.status !== "queued" ||
      activeTurn ||
      actionBusy ||
      projectTransitioning ||
      !canPrompt ||
      !useNativeAcp
    )
      return;
    updateQueuedPrompt(next.id, "sending");
    const afterOrder =
      knownTurnOrder && observedOrder
        ? laterTurnOrder(observedOrder, knownTurnOrder)
          ? observedOrder
          : knownTurnOrder
        : (knownTurnOrder ?? observedOrder);
    void submit(
      next.text,
      next.attachedContext,
      next.bindingId as CuratedBindingId,
      next.modelId as CuratedModelId | null,
    )
      .then((result) => {
        if (result === "applied")
          setQueuedTurnBarrier({
            afterOrder,
            deadlineMs: Date.now() + QUEUED_TURN_START_TIMEOUT_MS,
          });
        updateQueuedPrompt(
          next.id,
          result === "applied" ? "deleted" : "blocked",
        );
      })
      .catch(() => updateQueuedPrompt(next.id, "blocked"));
  }, [
    actionBusy,
    activeTurn,
    canPrompt,
    knownTurnOrder,
    projectTransitioning,
    queuedPrompts,
    queuedTurnBarrier,
    observedOrder,
    setQueuedTurnBarrier,
    submit,
    updateQueuedPrompt,
    useNativeAcp,
  ]);
}
