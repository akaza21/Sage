import { lazy, Suspense } from "react";
import { DEVICE_TOOLBAR_HEIGHT } from "./browserDevice";
import type { BrowserDeviceState } from "./useBrowserDevice";

// The toolbar, its handles and their styles load only once a device is shown.
const BrowserDeviceToolbar = lazy(() =>
  import("./BrowserDeviceToolbar").then((module) => ({
    default: module.BrowserDeviceToolbar,
  })),
);
const BrowserDeviceHandles = lazy(() =>
  import("./BrowserDeviceToolbar").then((module) => ({
    default: module.BrowserDeviceHandles,
  })),
);

export function BrowserDeviceBar({ device }: { device: BrowserDeviceState }) {
  if (!device.size) return null;
  // The fallback holds the toolbar's row so the device keeps its size while
  // the toolbar loads.
  return (
    <Suspense
      fallback={<div style={{ flex: `0 0 ${DEVICE_TOOLBAR_HEIGHT}px` }} />}
    >
      <BrowserDeviceToolbar
        size={device.size}
        scalePercent={device.scalePercent}
        onChange={device.change}
        onClose={device.close}
      />
    </Suspense>
  );
}

export function BrowserDeviceGutter({
  device,
}: {
  device: BrowserDeviceState;
}) {
  if (!device.size || device.scalePercent === null) return null;
  return (
    <Suspense fallback={null}>
      <BrowserDeviceHandles
        size={device.size}
        scalePercent={device.scalePercent}
        onChange={device.change}
      />
    </Suspense>
  );
}
