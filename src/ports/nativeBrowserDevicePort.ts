import type { BrowserDevice } from "./nativeBrowserPort";
import { invokeNative } from "./nativeInvoke";

/** Why an update failed and what the tab holds after it: null once Rust
 * confirmed the reset, undefined when the answer could not be read. */
export interface BrowserDeviceFailure {
  message: string;
  device: BrowserDevice | null | undefined;
}

const MAX_MESSAGE_LENGTH = 512;

/** Applies the device toolbar's emulation to the tab, or resets it with
 * null. A rejection carries a {@link BrowserDeviceFailure}. */
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

export function readDeviceFailure(cause: unknown): BrowserDeviceFailure {
  if (typeof cause === "object" && cause !== null && "message" in cause) {
    const { message, device } = cause as Record<string, unknown>;
    if (typeof message === "string" && (device === null || isDevice(device)))
      return { message: message.slice(0, MAX_MESSAGE_LENGTH), device };
  }
  return {
    message: String(cause).slice(0, MAX_MESSAGE_LENGTH),
    device: undefined,
  };
}
