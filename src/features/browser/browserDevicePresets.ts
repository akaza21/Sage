import presets from "./browserDevicePresets.json";
import { RESPONSIVE, type BrowserDeviceSize } from "./browserDevice";

/** A device toolbar preset. Rust holds the same list with each device's
 * scale factor, touch and user agent, and admits only these ids and sizes. */
export interface BrowserDevicePreset {
  id: string;
  label: string;
  /** Zero for Responsive, whose size is free. */
  width: number;
  height: number;
}

export const DEVICE_PRESETS: readonly BrowserDevicePreset[] = presets;

export function devicePreset(id: string): BrowserDevicePreset | undefined {
  return DEVICE_PRESETS.find((preset) => preset.id === id);
}

/** Switching presets keeps the orientation; Responsive keeps the size. */
export function selectDevicePreset(
  current: BrowserDeviceSize,
  id: string,
): BrowserDeviceSize {
  const preset = devicePreset(id);
  if (!preset) return current;
  if (preset.id === RESPONSIVE) return { ...current, preset: RESPONSIVE };
  const landscape = current.width > current.height;
  const [short, long] = [
    Math.min(preset.width, preset.height),
    Math.max(preset.width, preset.height),
  ];
  return landscape
    ? { preset: preset.id, width: long, height: short }
    : { preset: preset.id, width: short, height: long };
}
