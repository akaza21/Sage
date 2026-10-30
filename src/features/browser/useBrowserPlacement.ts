import { useEffect, type RefObject } from "react";
import { hideBrowser, positionBrowser } from "../../ports/nativeBrowserPort";

export function useBrowserPlacement(
  tabId: string,
  frame: RefObject<HTMLDivElement | null>,
  surface: RefObject<HTMLElement | null>,
  onError: (message: string) => void,
  covered = false,
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
      // The page is a native window above Sage's DOM, so it hides while a
      // toolbar popover covers it, and a resize must not show it again.
      if (covered || center?.getAttribute("aria-hidden") === "true") {
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
    if (center) {
      visibilityObserver.observe(center, {
        attributes: true,
        attributeFilter: ["aria-hidden"],
      });
    }
    window.addEventListener("resize", place);
    place();
    return () => {
      active = false;
      cancelAnimationFrame(frameId);
      observer.disconnect();
      visibilityObserver.disconnect();
      window.removeEventListener("resize", place);
      void hideBrowser(tabId).catch(() => undefined);
    };
  }, [tabId, frame, surface, onError, covered]);
}
