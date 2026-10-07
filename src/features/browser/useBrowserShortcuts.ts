import { useEffect, useRef, type RefObject } from "react";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  dispatchBrowserChromeShortcut,
  type BrowserShortcut,
} from "./browserShortcuts";

interface BrowserShortcutIntent {
  tabId: string;
  action: BrowserShortcut;
}

export function useBrowserShortcuts(
  root: RefObject<HTMLElement | null>,
  tabId: string,
  loading: boolean,
  onAction: (action: BrowserShortcut) => void,
): void {
  const latest = useRef({ loading, onAction });
  useEffect(() => {
    latest.current = { loading, onAction };
  }, [loading, onAction]);

  useEffect(() => {
    const element = root.current;
    if (!element) return;
    const onKeyDown = (event: KeyboardEvent) => {
      dispatchBrowserChromeShortcut(
        element,
        event,
        latest.current.loading,
        latest.current.onAction,
      );
    };
    element.addEventListener("keydown", onKeyDown);
    return () => element.removeEventListener("keydown", onKeyDown);
  }, [root]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<BrowserShortcutIntent>("browser:shortcut", (event) => {
          if (active && event.payload.tabId === tabId) {
            latest.current.onAction(event.payload.action);
          }
        }),
      )
      .then((dispose) => {
        if (active) unlisten = dispose;
        else dispose();
      })
      .catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, [tabId]);
}
