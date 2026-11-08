import type {
  AcpUsageRequest,
  AcpUsageState,
  UsageWindow,
} from "../domain/acpUsage";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { isOneOf, isRecord, safeText } from "./acpDesktopGuards";

const MAX_WINDOWS = 8;

const bounded = (value: unknown, max: number): value is number =>
  Number.isSafeInteger(value) &&
  (value as number) >= 0 &&
  (value as number) <= max;

const nullable = (value: unknown, max: number) =>
  value === null || bounded(value, max);

function decodeWindow(value: unknown): UsageWindow | null {
  if (
    !isRecord(value) ||
    (value.label !== null && !safeText(value.label, 128)) ||
    !bounded(value.usedPercent, 100) ||
    !nullable(value.windowMinutes, 0xffff_ffff) ||
    !nullable(value.resetsAt, 253_402_300_799)
  )
    return null;
  return {
    label: value.label as string | null,
    usedPercent: value.usedPercent,
    windowMinutes: value.windowMinutes as number | null,
    resetsAt: value.resetsAt as number | null,
  };
}

export function decodeUsageState(value: unknown): AcpUsageState | null {
  if (!isRecord(value)) return null;
  if (isOneOf(value.kind, ["unsupported", "offline", "unavailable"] as const))
    return { kind: value.kind };
  const snapshot = value.snapshot;
  if (
    value.kind !== "usage" ||
    !isRecord(snapshot) ||
    (snapshot.plan !== null && !safeText(snapshot.plan, 128)) ||
    typeof snapshot.limitReached !== "boolean" ||
    !Array.isArray(snapshot.windows) ||
    snapshot.windows.length > MAX_WINDOWS
  )
    return null;
  const windows: UsageWindow[] = [];
  for (const candidate of snapshot.windows) {
    const window = decodeWindow(candidate);
    if (!window) return null;
    windows.push(window);
  }
  return {
    kind: "usage",
    snapshot: {
      plan: snapshot.plan as string | null,
      limitReached: snapshot.limitReached,
      windows,
    },
  };
}

export const acpUsageDesktop = {
  async read(request: AcpUsageRequest): Promise<AcpUsageState> {
    const state = decodeUsageState(await invokeAcp("get_acp_usage", request));
    if (!state)
      throw new AcpDesktopError({
        kind: "internal",
        message: "Usage limits returned an invalid bounded response.",
      });
    return state;
  },
};
