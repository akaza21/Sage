import type { listen } from "@tauri-apps/api/event";
import type { AcpGoalChangedV1 } from "../domain/acpGoal";
import { isRecord, safeCounter, safeIdentifier } from "./acpDesktopGuards";

export function decodeAcpGoalNotice(value: unknown): AcpGoalChangedV1 | null {
  if (!isRecord(value) || value.schemaVersion !== 1) return null;
  for (const key of [
    "sageProjectId",
    "sageChatId",
    "sageAgentSessionId",
  ] as const) {
    if (!safeIdentifier(value[key])) return null;
  }
  if (!safeCounter(value.runtimeEpoch, false)) return null;
  return value as unknown as AcpGoalChangedV1;
}

export function subscribeGoalChanges(
  onChange: (event: AcpGoalChangedV1) => void,
  listenEvent: typeof listen,
) {
  return listenEvent<unknown>("acp-goal-changed-v1", (event) => {
    const notice = decodeAcpGoalNotice(event.payload);
    if (notice) onChange(notice);
  });
}
