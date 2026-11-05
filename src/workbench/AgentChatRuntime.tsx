import { useWorkbenchStore } from "../app/store";
import { useMemo } from "react";
import { ChatQueueOwner } from "../features/chat/ChatQueueOwner";
import { AcpProjectionOwnerContext } from "../features/chat/acp/AcpProjectionOwnerContext";
import { useAcpChatProjection } from "../features/chat/acp/useAcpChatProjection";

function ChatProjectionOwner({
  chatId,
  pending,
}: {
  chatId: string;
  pending: boolean;
}) {
  useAcpChatProjection(true, chatId);
  const owned = useMemo(() => new Set([chatId]), [chatId]);
  return (
    <AcpProjectionOwnerContext value={owned}>
      {pending ? <ChatQueueOwner chatId={chatId} /> : null}
    </AcpProjectionOwnerContext>
  );
}

export default function AgentChatRuntime({
  ids,
  visible,
}: {
  ids: readonly string[];
  visible: boolean;
}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const chats = useWorkbenchStore((state) => state.chatUiById);
  return ids.map((id) => {
    const chat = chats[id];
    // Only the FIFO head can advance; a blocked head needs explicit retry.
    // A later item may still be sending through Steer.
    const pending = Boolean(
      chat?.queuedPrompts?.[0]?.status === "queued" ||
      chat?.queuedPrompts?.some((prompt) => prompt.status === "sending") ||
      chat?.queuedTurnBarrier,
    );
    return tabs.some((tab) => tab.id === id && tab.kind === "chat") &&
      (visible || pending) ? (
      <ChatProjectionOwner key={id} chatId={id} pending={pending} />
    ) : null;
  });
}
