import { useAgentChatFocus } from "../../app/agentChatOwner";
import { useWorkbenchStore } from "../../app/store";
import { useAcpChatProjection } from "../chat/acp/useAcpChatProjection";

export function HarnessDiagnostics() {
  const chatId = useAgentChatFocus();
  const page = useAcpChatProjection(!!chatId, chatId);
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const session = page?.sessions.find(
    (entry) => entry.id === page.currentSessionId,
  );
  const harness = harnesses.find(
    (entry) => entry.bindingId === session?.bindingId,
  );
  const notices =
    page?.sessionNotices?.filter(
      (notice) => notice.diagnostic !== false && notice.level !== "error",
    ) ?? [];
  if (!chatId || page?.chatId !== chatId || !harness || !notices.length)
    return null;
  return (
    <section aria-label={`${harness.name} diagnostics`}>
      <h3>{harness.name} diagnostics</h3>
      <ul>
        {notices.map((notice, index) => (
          <li key={index}>{notice.text}</li>
        ))}
      </ul>
    </section>
  );
}
