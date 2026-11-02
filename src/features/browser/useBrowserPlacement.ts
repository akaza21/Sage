import { useEffect, type RefObject } from "react";
import {
  nativePageOverlapped,
  subscribeNativePageLayers,
} from "../../ui/nativePageLayers";
import {
  coverBrowser,
  hideBrowser,
  positionBrowser,
} from "../../ports/nativeBrowserPort";

export function useBrowserPlacement(
  tabId: string,
  frame: RefObject<HTMLDivElement | null>,
  surface: RefObject<HTMLElement | null>,
  onError: (message: string) => void,
) {
  useEffect(() => {
    const element = frame.current;
    if (!element) return;
    const center = element.closest(".center-region");
    const agentCanvas = element.closest(".agent-artifact-pane__canvas");
    let active = true;
    let frameId = 0;
    const reportError = (cause: unknown) => {
      if (active) onError(String(cause).slice(0, 512));
    };
    const place = () => {
      cancelAnimationFrame(frameId);
      // The page is a native window above Sage's DOM, so it hides while the
      // centre pane is hidden, and a resize must not show it again.
      if (center?.getAttribute("aria-hidden") === "true") {
        void hideBrowser(tabId).catch(reportError);
        return;
      }
      frameId = requestAnimationFrame(() => {
        const rect = element.getBoundingClientRect();
        const canvasRect = agentCanvas?.getBoundingClientRect();
        const toolbarBottom = agentCanvas
          ? (surface.current
              ?.querySelector(".browser-surface__toolbar")
              ?.getBoundingClientRect().bottom ??
            canvasRect?.top ??
            rect.top)
          : rect.top;
        const left = canvasRect
          ? Math.max(rect.left, canvasRect.left)
          : rect.left;
        const top = canvasRect
          ? Math.max(rect.top, canvasRect.top, toolbarBottom)
          : rect.top;
        const right = canvasRect
          ? Math.min(rect.right, canvasRect.right)
          : rect.right;
        const bottom = canvasRect
          ? Math.min(rect.bottom, canvasRect.bottom)
          : rect.bottom;
        if (right - left < 1 || bottom - top < 1) {
          if (agentCanvas) void hideBrowser(tabId).catch(reportError);
          return;
        }
        if (
          nativePageOverlapped(
            new DOMRect(left, top, right - left, bottom - top),
          )
        ) {
          void coverBrowser(tabId).catch(reportError);
          return;
        }
        void positionBrowser(tabId, {
          x: left,
          y: top,
          width: right - left,
          height: bottom - top,
        }).catch(reportError);
      });
    };
    const observer = new ResizeObserver(place);
    observer.observe(element);
    const visibilityObserver = new MutationObserver(place);
    if (center)
      visibilityObserver.observe(center, {
        attributes: true,
        attributeFilter: ["aria-hidden"],
      });
    const unsubscribe = subscribeNativePageLayers(place);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    place();
    return () => {
      active = false;
      cancelAnimationFrame(frameId);
      observer.disconnect();
      unsubscribe();
      visibilityObserver.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
      void hideBrowser(tabId).catch(() => undefined);
    };
  }, [tabId, frame, surface, onError]);
}
