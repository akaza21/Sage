import type { GetAcpChatPageRequest } from "../domain/acp";
import { decodeAcpGoal, type AcpGoalChangedV1 } from "../domain/acpGoal";
import { acpDesktop } from "./acpDesktop";

export function getGoalChatPage(request: GetAcpChatPageRequest) {
  return acpDesktop.getChatPage(request, decodeAcpGoal);
}

export async function subscribeGoalChanges(
  onChange: (event: AcpGoalChangedV1) => void,
): Promise<() => void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
    return () => undefined;
  // Both chunks load together. Destructuring the event import where it is
  // awaited lets the bundler leave out the event exports Sage never uses.
  const subscription = import("./acpGoalSubscription");
  const { listen } = await import("@tauri-apps/api/event");
  const { subscribeGoalChanges } = await subscription;
  return subscribeGoalChanges(onChange, listen);
}
