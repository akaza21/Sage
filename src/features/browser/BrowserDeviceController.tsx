import { useCallback, useLayoutEffect, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { BrowserDevice } from "../../ports/nativeBrowserPort";
import {
  BrowserDeviceHandles,
  BrowserDeviceToolbar,
} from "./BrowserDeviceToolbar";
import { drawnDevice, useBrowserDevice } from "./useBrowserDevice";

/** Draws the page as a centred device of this size in the frame's gutter
 * (browser-device.css); the returned cleanup gives the frame back. */
function markDeviceFrame(frame: HTMLElement, width: number, height: number) {
  const sizes = Object.entries({
    "--browser-device-width": `${width}px`,
    "--browser-device-height": `${height}px`,
  });
  frame.dataset.device = "";
  for (const [name, value] of sizes) frame.style.setProperty(name, value);
  return () => {
    delete frame.dataset.device;
    for (const [name] of sizes) frame.style.removeProperty(name);
  };
}

/**
 * The device toolbar (BF-7a), loaded the first time the surface shows it or
 * restores a tab's emulation. It owns the toolbar row, the grips in the
 * page gutter and the frame's device size; the surface keeps only `open`.
 */
export function BrowserDeviceController({
  tabId,
  open,
  restored,
  frame,
  onOpenChange,
  onError,
}: {
  tabId: string;
  open: boolean;
  restored: BrowserDevice | null;
  frame: RefObject<HTMLDivElement | null>;
  onOpenChange: (open: boolean) => void;
  onError: (message: string) => void;
}) {
  const device = useBrowserDevice({
    tabId,
    open,
    restored,
    frame,
    onOpenChange,
    onError,
  });
  const drawn = drawnDevice(device);
  const drawnWidth = drawn?.width;
  const drawnHeight = drawn?.height;
  // The surface owns the frame; the device only marks it and sizes the page.
  useLayoutEffect(() => {
    const element = frame.current;
    if (!element || drawnWidth === undefined || drawnHeight === undefined)
      return;
    return markDeviceFrame(element, drawnWidth, drawnHeight);
  }, [frame, drawnWidth, drawnHeight]);
  const [viewport, setViewport] = useState<Element | null>(null);
  const findViewport = useCallback(
    (row: HTMLDivElement | null) =>
      setViewport(
        row
          ?.closest(".browser-surface")
          ?.querySelector(".browser-surface__viewport") ?? null,
      ),
    [],
  );
  if (!device.shown) return null;
  const { size, scalePercent, change } = device;
  // The row holds its height while Responsive measures the room it has.
  return (
    <div ref={findViewport} className="browser-surface__device-slot">
      {size ? (
        <BrowserDeviceToolbar
          size={size}
          scalePercent={scalePercent}
          onChange={change}
          onClose={() => onOpenChange(false)}
        />
      ) : null}
      {viewport && size && scalePercent !== null
        ? createPortal(
            <BrowserDeviceHandles
              size={size}
              scalePercent={scalePercent}
              onChange={change}
            />,
            viewport,
          )
        : null}
    </div>
  );
}
