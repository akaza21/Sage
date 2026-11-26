import { useEffect } from "react";
import type { AttachedContext } from "../../domain/attachedContext";
import type {
  AcpChatPage,
  CuratedBindingId,
  CuratedModelId,
} from "../../domain/acp";
import type {
  ChatTurnOrder,
  QueuedChatPrompt,
} from "../../domain/chatOwnership";
import { laterTurnOrder } from "./useChatQueueState";

const QUEUED_TURN_START_TIMEOUT_MS = 30_000;

export default function ChatQueueExecution({
  queuedPrompts,
  queuedTurnBarrier,
  knownTurnOrder,
  activeTurn,
  actionBusy,
  projectTransitioning,
  canPrompt,
  useNativeAcp,
  page,
  observedOrder,
  submit,
  updateQueuedPrompt,
  setQueuedTurnBarrier,
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
  observedOrder: ChatTurnOrder | null;
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
}) {
  // After a sign-in failure, wait for a newer turn (a retry) before draining.
  const last = page?.turns.at(-1);
  const hold =
    last?.sessionId === page?.currentSessionId &&
    last?.terminalReason?.kind === "authRequired";
  useEffect(() => {
    if (queuedTurnBarrier !== null) {
      if (
        !observedOrder ||
        (queuedTurnBarrier.afterOrder &&
          !laterTurnOrder(observedOrder, queuedTurnBarrier.afterOrder))
      ) {
        const remainingMs = queuedTurnBarrier.deadlineMs - Date.now();
        const blockNext = () => {
          const next = queuedPrompts[0];
          if (next?.status === "queued") updateQueuedPrompt(next.id, "blocked");
          setQueuedTurnBarrier(null);
        };
        if (remainingMs <= 0) {
          blockNext();
          return;
        }
        const timer = setTimeout(blockNext, remainingMs);
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
      !useNativeAcp ||
      hold
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
    hold,
    setQueuedTurnBarrier,
    submit,
    updateQueuedPrompt,
    useNativeAcp,
  ]);
  return null;
}
