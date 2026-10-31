import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import {
  browserSetDevice,
  type BrowserDevice,
} from "../../ports/nativeBrowserPort";
import {
  DEVICE_TOOLBAR_HEIGHT,
  deviceArea,
  drawnDeviceSize,
  fitScalePercent,
  responsiveDevice,
  type BrowserDeviceArea,
  type BrowserDeviceSize,
} from "./browserDevice";

export interface BrowserDeviceState {
  size: BrowserDeviceSize | null;
  /** The percent the device is drawn at; null while closed or unmeasured. */
  scalePercent: number | null;
  open: () => void;
  close: () => void;
  change: (size: BrowserDeviceSize) => void;
  frameClass: string;
  frameStyle: CSSProperties | undefined;
}

interface DeviceUpdate {
  tabId: string;
  device: BrowserDevice | null;
}

interface DeviceQueue {
  running: boolean;
  /** The latest update still to send; undefined when none waits. */
  next: DeviceUpdate | undefined;
}

/** Sends updates to Rust one at a time; only the latest waiting one is
 * sent, so a drag never queues more than one call. Each update names its
 * own tab, so a tab switch cannot send one tab's device to another. */
function enqueue(
  queue: DeviceQueue,
  update: DeviceUpdate,
  onFailure: (update: DeviceUpdate, message: string) => void,
) {
  queue.next = update;
  if (queue.running) return;
  queue.running = true;
  void (async () => {
    while (queue.next !== undefined) {
      const job = queue.next;
      queue.next = undefined;
      try {
        await browserSetDevice(job.tabId, job.device);
      } catch (cause) {
        // Rust reset the tab after a failed apply; a change still waiting
        // for that tab would apply again what the user just lost.
        // The await above may have queued one; TypeScript cannot see that.
        const waiting = queue.next as DeviceUpdate | undefined;
        if (job.device && waiting?.tabId === job.tabId) queue.next = undefined;
        onFailure(job, String(cause).slice(0, 512));
      }
    }
    queue.running = false;
  })();
}

/**
 * The device toolbar's state for one tab. Rust holds the applied emulation,
 * so a remounted surface restores it from the tab's first status.
 */
export function useBrowserDevice(
  tabId: string,
  frame: RefObject<HTMLDivElement | null>,
  /** The tab's status: its device restores once, from this tab's status. */
  status: { tabId: string; device?: BrowserDevice | null } | null,
  onError: (message: string) => void,
): BrowserDeviceState {
  const [size, setSize] = useState<BrowserDeviceSize | null>(null);
  const [area, setArea] = useState<BrowserDeviceArea | null>(null);
  const [restoredTab, setRestoredTab] = useState<string | null>(null);
  const [shownTab, setShownTab] = useState(tabId);
  const queue = useRef<DeviceQueue>({ running: false, next: undefined });
  const currentTab = useRef(tabId);
  useEffect(() => {
    currentTab.current = tabId;
  }, [tabId]);
  // The pane can show another tab without remounting: start that tab from
  // its own status, never from the last tab's device.
  if (shownTab !== tabId) {
    setShownTab(tabId);
    setSize(null);
    setArea(null);
    setRestoredTab(null);
  }
  const restored =
    status?.tabId === tabId ? (status.device ?? null) : undefined;
  if (restored !== undefined && restoredTab !== tabId) {
    setRestoredTab(tabId);
    if (restored)
      setSize({
        preset: restored.preset,
        width: restored.width,
        height: restored.height,
      });
  }

  const opened = size !== null;
  useEffect(() => {
    const element = frame.current;
    if (!opened || !element) return;
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
  }, [opened, frame]);

  const onFailure = (update: DeviceUpdate, message: string) => {
    if (update.tabId !== currentTab.current) return;
    onError(message);
    if (update.device) setSize(null);
  };
  const scalePercent = size && area ? fitScalePercent(size, area) : null;
  const preset = size?.preset;
  const width = size?.width;
  const height = size?.height;
  useEffect(() => {
    if (
      preset === undefined ||
      width === undefined ||
      height === undefined ||
      scalePercent === null
    )
      return;
    enqueue(
      queue.current,
      { tabId, device: { preset, width, height, scalePercent } },
      onFailure,
    );
  }, [tabId, preset, width, height, scalePercent, onFailure]);

  const drawn =
    size && scalePercent !== null ? drawnDeviceSize(size, scalePercent) : null;
  return {
    size,
    scalePercent,
    open: () => {
      const element = frame.current;
      if (!element) return;
      // Responsive opens at the room left once its toolbar takes its row.
      const { width, height } = element.getBoundingClientRect();
      setSize(
        responsiveDevice(
          deviceArea({ width, height: height - DEVICE_TOOLBAR_HEIGHT }),
        ),
      );
    },
    close: () => {
      setSize(null);
      setArea(null);
      enqueue(queue.current, { tabId, device: null }, onFailure);
    },
    change: setSize,
    frameClass: drawn ? " browser-surface__frame--device" : "",
    frameStyle: drawn
      ? ({
          "--browser-device-width": `${drawn.width}px`,
          "--browser-device-height": `${drawn.height}px`,
        } as CSSProperties)
      : undefined,
  };
}
