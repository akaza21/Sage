import { listen } from "@tauri-apps/api/event";
import type {
  BrowserAnnotateChange,
  BrowserNavigation,
  BrowserPermissionSnapshot,
} from "../../ports/nativeBrowserPort";
import type { BrowserDownload } from "../../ports/nativeBrowserDownloadsPort";

interface BrowserTabEvents {
  "browser:navigation": BrowserNavigation;
  "browser:agent-control": { tabId: string };
  "browser:permissions": BrowserPermissionSnapshot;
  "browser:downloads": { tabId: string; downloads: BrowserDownload[] };
  "browser:annotate": BrowserAnnotateChange;
}

/** Owns a tab listener, including registration that finishes after disposal. */
export function subscribeBrowserTab<Event extends keyof BrowserTabEvents>(
  name: Event,
  tabId: string,
  accept: (payload: BrowserTabEvents[Event], initial: boolean) => void,
  read?: () => Promise<BrowserTabEvents[Event] | void>,
  onError?: (message: string) => void,
): () => void {
  let active = true;
  let remove: (() => void) | undefined;
  void (async () => {
    const stop = await listen<BrowserTabEvents[Event]>(name, ({ payload }) => {
      if (active && payload.tabId === tabId) accept(payload, false);
    });
    if (!active) return stop();
    remove = stop;
    const initial = await read?.();
    if (active && initial) accept(initial, true);
  })().catch((cause) => {
    if (active) onError?.(String(cause).slice(0, 512));
  });
  return () => {
    active = false;
    remove?.();
  };
}
