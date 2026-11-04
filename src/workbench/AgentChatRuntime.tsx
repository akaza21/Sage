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
    const pending = Boolean(
      chats[id]?.queuedPrompts?.length || chats[id]?.queuedTurnBarrier,
    );
    return tabs.some((tab) => tab.id === id && tab.kind === "chat") &&
      (visible || pending) ? (
      <ChatProjectionOwner key={id} chatId={id} pending={pending} />
    ) : null;
  });
}
