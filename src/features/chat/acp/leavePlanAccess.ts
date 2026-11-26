import {
  chooseChatAccess,
  loadChatAccess,
  useChatAccessStore,
} from "../../../app/sessionAccessActions";
import type { PermissionDecision } from "../../../domain/acp";

/**
 * An approved mode switch (a plan) leaves plan mode, so a chat in Plan
 * access follows it: to asking before edits, or to accepting edits when the
 * person chose that. Otherwise the chat's next message would put the agent
 * back into plan mode.
 */
export async function leavePlanAccess(
  sessionId: string,
  decision: PermissionDecision,
): Promise<void> {
  const next =
    decision === "allowOnce"
      ? "ask"
      : decision === "allowAlways"
        ? "acceptEdits"
        : null;
  if (!next) return;
  if (!(sessionId in useChatAccessStore.getState().modes))
    await loadChatAccess(sessionId);
  if (useChatAccessStore.getState().modes[sessionId] !== "plan") return;
  await chooseChatAccess(sessionId, next);
}
