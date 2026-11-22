import type { GetAcpChatPageRequest } from "../domain/acp";
import { decodeAcpGoal, type AcpGoalChangedV1 } from "../domain/acpGoal";
import { acpDesktop } from "./acpDesktop";
import { isTauriRuntime } from "../ports/desktopRuntime";

export function getGoalChatPage(request: GetAcpChatPageRequest) {
  return acpDesktop.getChatPage(request, decodeAcpGoal);
}

export async function subscribeGoalChanges(
  onChange: (event: AcpGoalChangedV1) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) return () => undefined;
  const [{ listen }, { subscribeGoalChanges }] = await Promise.all([
    import("@tauri-apps/api/event"),
    import("./acpGoalSubscription"),
  ]);
  return subscribeGoalChanges(onChange, listen);
}
