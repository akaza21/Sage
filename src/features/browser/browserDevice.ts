/** What the toolbar asks for; the scale follows from the pane. */
export interface BrowserDeviceSize {
  preset: string;
  width: number;
  height: number;
}

export interface BrowserDeviceArea {
  width: number;
  height: number;
}

export const RESPONSIVE = "responsive";
/** The device toolbar's height, divider included (Codex ref 41). */
export const DEVICE_TOOLBAR_HEIGHT = 36;
export const MIN_DEVICE_SIDE = 50;
export const MAX_DEVICE_SIDE = 3840;
const MIN_SCALE_PERCENT = 10;
/** The gutter beside and below the device, where its resize handles sit
 * (Codex ref 41: 18 px, with the page flush under the device toolbar). */
const GUTTER_SIDE = 18;
const GUTTER_TOP = 0;
const GUTTER_BOTTOM = 18;

export function clampDeviceSide(value: number): number {
  if (!Number.isFinite(value)) return MIN_DEVICE_SIDE;
  return Math.min(
    MAX_DEVICE_SIDE,
    Math.max(MIN_DEVICE_SIDE, Math.round(value)),
  );
}

/** The room the device has inside the page frame, less the gutter. */
export function deviceArea(frame: BrowserDeviceArea): BrowserDeviceArea {
  return {
    width: Math.max(0, Math.floor(frame.width - 2 * GUTTER_SIDE)),
    height: Math.max(0, Math.floor(frame.height - GUTTER_TOP - GUTTER_BOTTOM)),
  };
}

/** Responsive opens at the size of the room it has, as in Codex. */
export function responsiveDevice(area: BrowserDeviceArea): BrowserDeviceSize {
  return {
    preset: RESPONSIVE,
    width: clampDeviceSide(area.width),
    height: clampDeviceSide(area.height),
  };
}

export function rotateDevice(current: BrowserDeviceSize): BrowserDeviceSize {
  return { ...current, width: current.height, height: current.width };
}

/** The whole percent the device is drawn at so it fits, at most 100%.
 * Null while the pane has no room to measure. */
export function fitScalePercent(
  size: BrowserDeviceSize,
  area: BrowserDeviceArea,
): number | null {
  if (area.width < 1 || area.height < 1) return null;
  const fit = Math.min(
    100,
    (area.width * 100) / size.width,
    (area.height * 100) / size.height,
  );
  return Math.max(MIN_SCALE_PERCENT, Math.floor(fit));
}

/** The size the device takes on screen at a scale. */
export function drawnDeviceSize(
  size: BrowserDeviceSize,
  scalePercent: number,
): BrowserDeviceArea {
  return {
    width: Math.max(1, Math.round((size.width * scalePercent) / 100)),
    height: Math.max(1, Math.round((size.height * scalePercent) / 100)),
  };
}
