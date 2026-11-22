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
  const [{ listen }, { subscribeGoalChanges }] = await Promise.all([
    import("@tauri-apps/api/event"),
    import("./acpGoalSubscription"),
  ]);
  return subscribeGoalChanges(onChange, listen);
}
