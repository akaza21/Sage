import { useEffect, useRef, type RefObject } from "react";
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
  hidden = false,
) {
  // A page that replaces the browser for a while (History) hides it for
  // real, ending Annotate, as a tab switch does.
  const hiddenRef = useRef(hidden);
  const placeRef = useRef<() => void>(() => undefined);
  useEffect(() => {
    hiddenRef.current = hidden;
    placeRef.current();
  }, [hidden]);
  useEffect(() => {
    const element = frame.current;
    if (!element) return;
    const center = element.closest(".center-region");
    const agentCanvas = element.closest(".agent-artifact-pane__canvas");
    let active = true;
    let frameId = 0;
    let lastPlacement: string | undefined;
    const reportError = (cause: unknown) => {
      if (active) onError(String(cause).slice(0, 512));
    };
    const update = (key: string, send: () => Promise<void>) => {
      if (lastPlacement === key) return;
      lastPlacement = key;
      void send().catch((cause) => {
        if (lastPlacement === key) lastPlacement = undefined;
        reportError(cause);
      });
    };
    const place = () => {
      cancelAnimationFrame(frameId);
      // The page is a native window above Sage's DOM, so it hides while the
      // centre pane is hidden, and a resize must not show it again.
      if (center?.getAttribute("aria-hidden") === "true" || hiddenRef.current) {
        update("hidden", () => hideBrowser(tabId));
        return;
      }
      frameId = requestAnimationFrame(() => {
        const rect = element.getBoundingClientRect();
        const canvasRect = agentCanvas?.getBoundingClientRect() ?? rect;
        const toolbarBottom = agentCanvas
          ? (surface.current
              ?.querySelector(".browser-surface__toolbar")
              ?.getBoundingClientRect().bottom ?? canvasRect.top)
          : rect.top;
        const left = Math.max(rect.left, canvasRect.left);
        const top = Math.max(rect.top, canvasRect.top, toolbarBottom);
        const right = Math.min(rect.right, canvasRect.right);
        const bottom = Math.min(rect.bottom, canvasRect.bottom);
        if (right - left < 1 || bottom - top < 1) {
          update("hidden", () => hideBrowser(tabId));
          return;
        }
        if (
          nativePageOverlapped(
            new DOMRect(left, top, right - left, bottom - top),
          )
        ) {
          update("covered", () => coverBrowser(tabId));
          return;
        }
        const bounds = {
          x: left,
          y: top,
          width: right - left,
          height: bottom - top,
        };
        update(JSON.stringify([bounds, window.devicePixelRatio]), () =>
          positionBrowser(tabId, bounds),
        );
      });
    };
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
    placeRef.current = place;
    watchResolution();
    const observer = new ResizeObserver(place);
    observer.observe(element);
    // A centred device moves without resizing when its frame does.
    if (element.parentElement) observer.observe(element.parentElement);
    const visibilityObserver = new MutationObserver(place);
    if (center)
      visibilityObserver.observe(center, {
        attributes: true,
        attributeFilter: ["aria-hidden"],
      });
    const unsubscribe = subscribeNativePageLayers(place);
    const onScroll = (event: Event) => {
      const target = event.target;
      if (
        target === window ||
        target === document ||
        (target instanceof Element && target.contains(element))
      )
        place();
    };
    window.addEventListener("resize", place);
    window.addEventListener("scroll", onScroll, true);
    place();
    return () => {
      active = false;
      placeRef.current = () => undefined;
      cancelAnimationFrame(frameId);
      observer.disconnect();
      unsubscribe();
      visibilityObserver.disconnect();
      resolution?.removeEventListener("change", onResolution);
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", onScroll, true);
      void hideBrowser(tabId).catch(() => undefined);
    };
  }, [tabId, frame, surface, onError]);
}
