import { useEffect, useRef, type RefObject } from "react";
import { useModalDialogOpen } from "../../ui/modalDialogState";
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
  covered = false,
  hidden = false,
) {
  const modalOpen = useModalDialogOpen();
  // A popover or shared modal places the page again without the cleanup
  // below, whose hide would end Annotate.
  const coveredRef = useRef(covered || modalOpen);
  // A page that replaces the browser for a while (History) hides it for
  // real, ending Annotate, as a tab switch does.
  const hiddenRef = useRef(hidden);
  const placeRef = useRef<() => void>(() => undefined);
  useEffect(() => {
    coveredRef.current = covered || modalOpen;
    hiddenRef.current = hidden;
    placeRef.current();
  }, [covered, hidden, modalOpen]);
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
      if (hiddenRef.current) {
        void hideBrowser(tabId).catch(reportError);
        return;
      }
      // Popovers and shared dialogs cover the native window, keeping Annotate on.
      if (coveredRef.current) {
        void coverBrowser(tabId).catch(reportError);
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
    placeRef.current = place;
    // Another display scale moves the native pixel grid even when the CSS
    // rectangle is unchanged, so the page is placed again on every change.
    let resolution: MediaQueryList | null = null;
    const watchResolution = () => {
      resolution?.removeEventListener("change", onResolution);
      resolution =
        window.matchMedia?.(`(resolution: ${window.devicePixelRatio}dppx)`) ??
        null;
      resolution?.addEventListener("change", onResolution);
    };
    const onResolution = () => {
      watchResolution();
      place();
    };
    watchResolution();
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
      placeRef.current = () => undefined;
      cancelAnimationFrame(frameId);
      observer.disconnect();
      visibilityObserver.disconnect();
      resolution?.removeEventListener("change", onResolution);
      window.removeEventListener("resize", place);
      void hideBrowser(tabId).catch(() => undefined);
    };
  }, [tabId, frame, surface, onError]);
}
