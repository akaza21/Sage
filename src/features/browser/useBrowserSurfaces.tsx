import { useRef, useState, type RefObject } from "react";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";

export function ClosedBrowserBoundary({ tabId }: { tabId: string }) {
  return (
    <LazyModuleBoundary
      load={loadClosedBrowser}
      label="Closed browser"
      render={(Surface) => <Surface tabId={tabId} />}
    />
  );
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
  import("./ClosedBrowserSurface").then((module) => module.loadBrowserNewTab());
export const loadClosedBrowser = () =>
  import("./ClosedBrowserSurface").then((module) => ({
    default: module.ClosedBrowserSurface,
  }));
export const loadBrowserFind = () =>
  import("./BrowserFindControls").then((module) => ({
    default: module.BrowserFindControls,
  }));
