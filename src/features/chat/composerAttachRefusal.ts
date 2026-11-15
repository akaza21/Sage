import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { isTauriRuntime } from "../../ports/nativeInvoke";

/**
 * Why files cannot be staged for this chat's next message, or null. Native
 * admission treats an absent capability as unsupported; a chat without a
 * session yet is checked again when its first message starts one.
 */
export function attachRefusal(chatId: string): string | null {
  if (!isTauriRuntime())
    return "Attaching files needs the Sage desktop app with an ACP harness.";
  const page = acpProjectionStoreFor(chatId).getState().page;
  const session = page?.sessions.find(
    (item) => item.id === page.currentSessionId,
  );
  return session && session.capabilities.promptResources !== true
    ? "This harness does not accept attached files (no embedded-context support)."
    : null;
}
