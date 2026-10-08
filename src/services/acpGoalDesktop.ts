import type { GetAcpChatPageRequest } from "../domain/acp";
import {
  decodeAcpGoal,
  decodeAcpGoalNotice,
  type AcpGoalChangedV1,
} from "../domain/acpGoal";
import { acpDesktop } from "./acpDesktop";

export function getGoalChatPage(request: GetAcpChatPageRequest) {
  return acpDesktop.getChatPage(request, decodeAcpGoal);
}

export async function subscribeGoalChanges(
  onChange: (event: AcpGoalChangedV1) => void,
): Promise<() => void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
    return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>("acp-goal-changed-v1", (event) => {
    const notice = decodeAcpGoalNotice(event.payload);
    if (notice) onChange(notice);
  });
}
