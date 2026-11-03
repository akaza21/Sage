import { describe, expect, it } from "vitest";
import {
  clampDeviceSide,
  deviceArea,
  drawnDeviceSize,
  fitScalePercent,
  MAX_DEVICE_SIDE,
  MIN_DEVICE_SIDE,
  responsiveDevice,
  rotateDevice,
} from "./browserDevice";
import { DEVICE_PRESETS, selectDevicePreset } from "./browserDevicePresets";

describe("device geometry", () => {
  it("keeps an 18 px gutter beside and below the device", () => {
    expect(deviceArea({ width: 878, height: 916 })).toEqual({
      width: 842,
      height: 898,
    });
    expect(deviceArea({ width: 20, height: 10 })).toEqual({
      width: 0,
      height: 0,
    });
  });

  it("opens Responsive at the room it has, within the admitted sides", () => {
    expect(responsiveDevice({ width: 842, height: 898 })).toEqual({
      preset: "responsive",
      width: 842,
      height: 898,
    });
    expect(responsiveDevice({ width: 10, height: 9000 })).toEqual({
      preset: "responsive",
      width: MIN_DEVICE_SIDE,
      height: MAX_DEVICE_SIDE,
    });
    expect(clampDeviceSide(Number.NaN)).toBe(MIN_DEVICE_SIDE);
    expect(clampDeviceSide(400.6)).toBe(401);
  });

  it("scales a device down to fit and never above 100%", () => {
    const device = { preset: "responsive", width: 922, height: 992 };
    expect(fitScalePercent(device, { width: 2000, height: 2000 })).toBe(100);
    expect(fitScalePercent(device, { width: 830, height: 2000 })).toBe(90);
    expect(fitScalePercent(device, { width: 2000, height: 496 })).toBe(50);
    expect(fitScalePercent(device, { width: 10, height: 10 })).toBe(10);
    expect(fitScalePercent(device, { width: 0, height: 500 })).toBeNull();
    expect(drawnDeviceSize(device, 90)).toEqual({ width: 830, height: 893 });
  });

  it("rotates by swapping the sides", () => {
    expect(
      rotateDevice({ preset: "iphone-se", width: 375, height: 667 }),
    ).toEqual({ preset: "iphone-se", width: 667, height: 375 });
  });
});

describe("device presets", () => {
  it("lists Responsive first and only fixed sizes after it", () => {
    expect(DEVICE_PRESETS[0]).toMatchObject({ id: "responsive", width: 0 });
    for (const preset of DEVICE_PRESETS.slice(1)) {
      expect(preset.width).toBeGreaterThan(0);
      expect(preset.height).toBeGreaterThan(0);
    }
  });

  it("switches presets in the current orientation and keeps Responsive's size", () => {
    const landscape = { preset: "responsive", width: 900, height: 500 };
    expect(selectDevicePreset(landscape, "iphone-se")).toEqual({
      preset: "iphone-se",
      width: 667,
      height: 375,
    });
    const portrait = { preset: "pixel-7", width: 412, height: 915 };
    expect(selectDevicePreset(portrait, "ipad-air")).toEqual({
      preset: "ipad-air",
      width: 820,
      height: 1180,
    });
    expect(selectDevicePreset(portrait, "responsive")).toEqual({
      preset: "responsive",
      width: 412,
      height: 915,
    });
    expect(selectDevicePreset(portrait, "unknown")).toBe(portrait);
  });
});
