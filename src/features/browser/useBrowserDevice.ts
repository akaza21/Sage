import { useEffect, useRef, useState, type RefObject } from "react";
import type { BrowserDevice } from "../../ports/nativeBrowserPort";
import {
  browserSetDevice,
  readDeviceFailure,
  type BrowserDeviceFailure,
} from "../../ports/nativeBrowserDevicePort";
import {
  deviceArea,
  drawnDeviceSize,
  fitScalePercent,
  responsiveDevice,
  type BrowserDeviceArea,
  type BrowserDeviceSize,
} from "./browserDevice";

/** Responsive before the pane is measured: it opens at the room it has. */
const FIT = "fit";

export interface BrowserDeviceState {
  /** The device shown; null while closed or until the pane is measured. */
  size: BrowserDeviceSize | null;
  /** Whether the toolbar takes its row, measured or not. */
  shown: boolean;
  /** The percent the device is drawn at; null while closed or unmeasured. */
  scalePercent: number | null;
  change: (size: BrowserDeviceSize) => void;
}

interface DeviceQueue {
  running: boolean;
  /** The latest update still to send for each tab, oldest tab first. */
  waiting: Map<string, BrowserDevice | null>;
}

type FailureHandler = (tabId: string, failure: BrowserDeviceFailure) => void;

/** Sends updates to Rust one at a time. Each tab keeps only its latest
 * waiting update, so a drag queues one call while another tab's reset is
 * never dropped; the map holds at most one entry per open tab. */
function enqueue(
  queue: DeviceQueue,
  tabId: string,
  device: BrowserDevice | null,
  onFailure: FailureHandler,
) {
  queue.waiting.delete(tabId);
  queue.waiting.set(tabId, device);
  if (queue.running) return;
  queue.running = true;
  void (async () => {
    for (const [next, update] of queue.waiting) {
      queue.waiting.delete(next);
      try {
        await browserSetDevice(next, update);
      } catch (cause) {
        // Rust says what the tab holds now; a change still waiting for that
        // tab would apply again what the failure just undid.
        queue.waiting.delete(next);
        onFailure(next, readDeviceFailure(cause));
      }
    }
    queue.running = false;
  })();
}

function sameDevice(a: BrowserDevice | null, b: BrowserDevice | null) {
  return (
    a === b ||
    (a !== null &&
      b !== null &&
      a.preset === b.preset &&
      a.width === b.width &&
      a.height === b.height &&
      a.scalePercent === b.scalePercent)
  );
}

/**
 * The device toolbar's state for the pane's tab. `open` is the surface's
 * choice; Rust holds the applied emulation, and the toolbar follows what a
 * failed update says the tab holds.
 */
export function useBrowserDevice({
  tabId,
  open,
  restored,
  frame,
  onOpenChange,
  onError,
}: {
  tabId: string;
  open: boolean;
  /** The emulation this tab's status reports, shown when the toolbar opens. */
  restored: BrowserDevice | null;
  frame: RefObject<HTMLDivElement | null>;
  onOpenChange: (open: boolean) => void;
  onError: (message: string) => void;
}): BrowserDeviceState {
  const [size, setSize] = useState<BrowserDeviceSize | typeof FIT | null>(null);
  const [area, setArea] = useState<BrowserDeviceArea | null>(null);
  const [shownTab, setShownTab] = useState(tabId);
  const queue = useRef<DeviceQueue>({ running: false, waiting: new Map() });
  /** What was last asked of Rust, and for which tab. */
  const sent = useRef<{ tabId: string; device: BrowserDevice | null }>({
    tabId,
    device: null,
  });
  // The pane can show another tab without remounting: that tab starts from
  // its own status, and the last tab keeps its emulation in Rust.
  if (shownTab !== tabId) {
    setShownTab(tabId);
    setSize(null);
    setArea(null);
  } else if (open && size === null) {
    setSize(
      restored
        ? {
            preset: restored.preset,
            width: restored.width,
            height: restored.height,
          }
        : FIT,
    );
  } else if (!open && size !== null) {
    setSize(null);
  } else if (size === FIT && area) {
    setSize(responsiveDevice(area));
  }

  const shown = size !== null;
  // A passive effect: the surface renders its frame after this toolbar, so
  // the frame's ref is only certain once the whole commit is done.
  useEffect(() => {
    const element = frame.current;
    if (!shown || !element) return;
    const measure = () => {
      const next = deviceArea(element.getBoundingClientRect());
      setArea((current) =>
        current?.width === next.width && current.height === next.height
          ? current
          : next,
      );
    };
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    measure();
    return () => observer.disconnect();
  }, [shown, frame]);

  const device = size === FIT ? null : size;
  const scalePercent = device && area ? fitScalePercent(device, area) : null;

  // The latest render's handler, so a failure reads the current tab and
  // callbacks without making the apply effect depend on them.
  const failed = useRef<FailureHandler>(() => undefined);
  useEffect(() => {
    failed.current = (failedTab, failure) => {
      if (failedTab !== tabId) return;
      onError(failure.message);
      // Unreadable: the toolbar keeps what it shows.
      if (failure.device === undefined) return;
      sent.current = { tabId, device: failure.device };
      if (failure.device === null) {
        setSize(null);
        onOpenChange(false);
      } else {
        // A close that failed reopens: the page may still be emulated.
        const { preset, width, height } = failure.device;
        setSize({ preset, width, height });
        onOpenChange(true);
      }
    };
  });
  const preset = device?.preset;
  const width = device?.width;
  const height = device?.height;
  useEffect(() => {
    let wanted: BrowserDevice | null;
    if (!shown) wanted = null;
    else if (
      preset === undefined ||
      width === undefined ||
      height === undefined ||
      scalePercent === null
    )
      return; // Not measured yet: nothing to send.
    else wanted = { preset, width, height, scalePercent };
    const last = sent.current.tabId === tabId ? sent.current.device : null;
    if (sameDevice(wanted, last)) return;
    sent.current = { tabId, device: wanted };
    enqueue(queue.current, tabId, wanted, (failedTab, failure) =>
      failed.current(failedTab, failure),
    );
  }, [tabId, shown, preset, width, height, scalePercent]);

  return {
    size: device,
    shown,
    scalePercent,
    change: setSize,
  };
}

/** The drawn device's size in CSS px, for the frame. */
export function drawnDevice(state: BrowserDeviceState) {
  return state.size && state.scalePercent !== null
    ? drawnDeviceSize(state.size, state.scalePercent)
    : null;
}
