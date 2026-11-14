import type { BrowserDevice } from "./nativeBrowserPort";
import { invokeNative } from "./nativeInvoke";

/** What the tab's page holds after a failed update: a device, nothing once
 * Rust confirmed the reset, `"unconfirmed"` when a reset step failed, or
 * undefined when the answer could not be read. */
export type BrowserDeviceHeld =
  BrowserDevice | null | "unconfirmed" | undefined;

export interface BrowserDeviceFailure {
  message: string;
  held: BrowserDeviceHeld;
}

const MAX_MESSAGE_LENGTH = 512;

export async function browserSetDevice(
  tabId: string,
  device: BrowserDevice | null,
): Promise<void> {
  try {
    await invokeNative("browser_set_device", { tabId, device });
  } catch (cause) {
    throw readDeviceFailure(cause);
  }
}

function isDevice(value: unknown): value is BrowserDevice {
  if (typeof value !== "object" || value === null) return false;
  const { preset, width, height, scalePercent } = value as Record<
    string,
    unknown
  >;
  return (
    typeof preset === "string" &&
    [width, height, scalePercent].every(Number.isSafeInteger)
  );
}

function readHeld(value: unknown): BrowserDeviceHeld {
  if (typeof value !== "object" || value === null) return undefined;
  const { state, device } = value as Record<string, unknown>;
  if (state === "unconfirmed") return "unconfirmed";
  if (state === "known" && (device === null || isDevice(device))) return device;
  return undefined;
}

export function readDeviceFailure(cause: unknown): BrowserDeviceFailure {
  if (typeof cause === "object" && cause !== null) {
    const { message, held } = cause as Record<string, unknown>;
    if (typeof message === "string")
      return {
        message: message.slice(0, MAX_MESSAGE_LENGTH),
        held: readHeld(held),
      };
  }
  return {
    message: String(cause).slice(0, MAX_MESSAGE_LENGTH),
    held: undefined,
  };
}
