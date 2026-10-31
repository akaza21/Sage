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

const UNCONFIRMED_HINT =
  "The page may still be partly emulated; change or close the device toolbar to try again";

/** Responsive before the pane is measured: it opens at the room it has. */
const FIT = "fit";

export interface BrowserDeviceState {
  /** The device shown; null while closed or until the pane is measured. */
  size: BrowserDeviceSize | null;
  /** Whether the toolbar takes its row, measured or not. */
  shown: boolean;
  /** The percent the device is drawn at; null while closed or unmeasured. */
  scalePercent: number | null;
  /** A reset step failed: the page may not match what the toolbar shows. */
  unconfirmed: boolean;
  change: (size: BrowserDeviceSize) => void;
}

interface DeviceQueue {
  running: boolean;
  /** The latest update still to send for each tab, oldest tab first. */
  waiting: Map<string, BrowserDevice | null>;
}

/** How an update ended: undefined when it applied. `resetWaiting` says a
 * reset for the same tab is still queued and will report its own result. */
type SettleHandler = (
  tabId: string,
  failure?: BrowserDeviceFailure & { resetWaiting: boolean },
) => void;

/** Sends updates to Rust one at a time. Each tab keeps only its latest
 * waiting update, so a drag queues one call while another tab's reset is
 * never dropped; the map holds at most one entry per open tab. */
function enqueue(
  queue: DeviceQueue,
  tabId: string,
  device: BrowserDevice | null,
  onSettled: SettleHandler,
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
        onSettled(next);
      } catch (cause) {
        // A waiting reset always goes: only it can clear what may be left.
        // A waiting change would apply again what the failure just undid.
        const resetWaiting = queue.waiting.get(next) === null;
        if (!resetWaiting) queue.waiting.delete(next);
        onSettled(next, { ...readDeviceFailure(cause), resetWaiting });
      }
    }
    queue.running = false;
  })();
}

/** Sent to Rust last when the page's state is not known: any update differs. */
const UNCONFIRMED = "unconfirmed";
type SentDevice = BrowserDevice | null | typeof UNCONFIRMED;

function sameDevice(a: BrowserDevice | null, b: SentDevice) {
  return (
    a === b ||
    (a !== null &&
      b !== null &&
      b !== UNCONFIRMED &&
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
  /** What this tab's status reports, used the first time the toolbar opens
   * for the tab: its device, or `"unconfirmed"` when a reset step failed. */
  restored: BrowserDevice | typeof UNCONFIRMED | null;
  frame: RefObject<HTMLDivElement | null>;
  onOpenChange: (open: boolean) => void;
  onError: (message: string) => void;
}): BrowserDeviceState {
  const [size, setSize] = useState<BrowserDeviceSize | typeof FIT | null>(null);
  const [area, setArea] = useState<BrowserDeviceArea | null>(null);
  const [shownTab, setShownTab] = useState(tabId);
  /** Whether a reset step failed on this tab since its last good update. */
  const [unconfirmed, setUnconfirmed] = useState(false);
  /** The tab whose status the toolbar has restored from, once per tab. */
  const [restoredTab, setRestoredTab] = useState<string | null>(null);
  /** Restored unconfirmed: nothing is sent until the user picks a size or
   * closes, so the toolbar never changes a page it cannot vouch for. */
  const [awaitingUser, setAwaitingUser] = useState(false);
  const queue = useRef<DeviceQueue>({ running: false, waiting: new Map() });
  /** What was last asked of Rust, or found on the page, and for which tab. */
  const sent = useRef<{ tabId: string; device: SentDevice }>({
    tabId,
    device: null,
  });
  // The pane can show another tab without remounting: that tab starts from
  // its own status, and the last tab keeps its emulation in Rust.
  if (shownTab !== tabId) {
    setShownTab(tabId);
    setSize(null);
    setArea(null);
    setUnconfirmed(false);
    setRestoredTab(null);
    setAwaitingUser(false);
  } else if (open && size === null) {
    const restore = restoredTab === tabId ? null : restored;
    setRestoredTab(tabId);
    if (restore === UNCONFIRMED) {
      setUnconfirmed(true);
      setAwaitingUser(true);
    }
    setSize(
      restore && restore !== UNCONFIRMED
        ? {
            preset: restore.preset,
            width: restore.width,
            height: restore.height,
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

  // The latest render's handler, so a result reads the current tab and
  // callbacks without making the apply effect depend on them.
  const settled = useRef<SettleHandler>(() => undefined);
  useEffect(() => {
    settled.current = (settledTab, failure) => {
      if (settledTab !== tabId) return;
      if (!failure) {
        if (sent.current.tabId === tabId && sent.current.device !== UNCONFIRMED)
          setUnconfirmed(false);
        return;
      }
      const { held, message, resetWaiting } = failure;
      if (held !== "unconfirmed") onError(message);
      // A queued reset reports for itself; unreadable keeps what is shown.
      if (resetWaiting || held === undefined) return;
      sent.current = { tabId, device: held };
      if (held === "unconfirmed") {
        // Not a guess at what is on: the toolbar says so, and its next
        // change or close is sent whatever it shows.
        setUnconfirmed(true);
        onError(`${message}. ${UNCONFIRMED_HINT}`);
      } else if (held === null) {
        setUnconfirmed(false);
        setSize(null);
        onOpenChange(false);
      } else {
        // A failed close of a known device reopens on it.
        const { preset, width, height } = held;
        setUnconfirmed(false);
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
    else if (awaitingUser)
      return; // Restored unconfirmed: wait for the user.
    else wanted = { preset, width, height, scalePercent };
    // Restored unconfirmed, the page may hold anything: a close is sent.
    const last = awaitingUser
      ? UNCONFIRMED
      : sent.current.tabId === tabId
        ? sent.current.device
        : null;
    if (sameDevice(wanted, last)) return;
    sent.current = { tabId, device: wanted };
    enqueue(queue.current, tabId, wanted, (settledTab, failure) =>
      settled.current(settledTab, failure),
    );
  }, [tabId, shown, preset, width, height, scalePercent, awaitingUser]);

  return {
    size: device,
    shown,
    scalePercent,
    unconfirmed,
    change: (next) => {
      setAwaitingUser(false);
      setSize(next);
    },
  };
}

/** The drawn device's size in CSS px, for the frame. */
export function drawnDevice(state: BrowserDeviceState) {
  return state.size && state.scalePercent !== null
    ? drawnDeviceSize(state.size, state.scalePercent)
    : null;
}
