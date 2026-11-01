import { useEffect, useRef, useState, type RefObject } from "react";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import {
  browserShortcutFor,
  type BrowserShortcut,
} from "./browserShortcutRegistry";

export type { BrowserShortcut } from "./browserShortcutRegistry";

export function dispatchBrowserChromeShortcut(
  root: HTMLElement,
  event: KeyboardEvent,
  loading: boolean,
  dispatch: (action: BrowserShortcut) => void,
): void {
  if (!(event.target instanceof Node) || !root.contains(event.target)) return;
  if (
    event.key === "Escape" &&
    (event.target as Element).closest?.(
      "[role=menu],[role=dialog],.browser-surface__find",
    )
  )
    return;
  const action = browserShortcutFor(event, loading);
  if (!action) return;
  event.preventDefault();
  event.stopPropagation();
  dispatch(action);
}

interface BrowserShortcutActions {
  address: HTMLInputElement | null;
  control: (action: "back" | "forward" | "reload" | "stop") => void;
  findOpen: () => void;
  zoom: (delta: number) => void;
  zoomPercent: number;
  newTab: () => void;
  closeTab: () => void;
}

export function performBrowserShortcut(
  action: BrowserShortcut,
  actions: BrowserShortcutActions,
): void {
  switch (action) {
    case "focusAddress":
      actions.address?.focus();
      actions.address?.select();
      break;
    case "reload":
    case "back":
    case "forward":
    case "stop":
      actions.control(action);
      break;
    case "find":
      actions.findOpen();
      break;
    case "zoomIn":
      actions.zoom(25);
      break;
    case "zoomOut":
      actions.zoom(-25);
      break;
    case "zoomReset":
      actions.zoom(100 - actions.zoomPercent);
      break;
    case "newTab":
      actions.newTab();
      break;
    case "closeTab":
      actions.closeTab();
      break;
  }
}

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

/** Menu and native shortcuts share the same focus and dismissal lifecycle. */
export function useBrowserFind(
  surface: RefObject<HTMLElement | null>,
  address: RefObject<HTMLInputElement | null>,
  stop: () => void,
) {
  const [findOpen, setFindOpen] = useState(false);
  const findField = useRef<HTMLInputElement>(null);
  const launcher = useRef<HTMLElement | null>(null);
  return {
    findOpen,
    findField,
    openFind: () => {
      if (!findOpen) {
        const active = document.activeElement;
        launcher.current =
          active instanceof HTMLElement && surface.current?.contains(active)
            ? active
            : address.current;
        setFindOpen(true);
      } else {
        findField.current?.focus();
        findField.current?.select();
      }
    },
    closeFind: () => {
      setFindOpen(false);
      (launcher.current?.isConnected
        ? launcher.current
        : address.current
      )?.focus();
      stop();
    },
  };
}

export const loadBrowserNewTab = () =>
  import("./browserNewTabLoader").then((module) => module.loadBrowserNewTab());
export const loadClosedBrowser = () =>
  import("./ClosedBrowserSurface").then((module) => ({
    default: module.ClosedBrowserSurface,
  }));
export const loadBrowserFind = () =>
  import("./BrowserFindControls").then((module) => ({
    default: module.BrowserFindControls,
  }));
