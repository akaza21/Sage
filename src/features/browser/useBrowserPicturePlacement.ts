import { useLayoutEffect, type RefObject } from "react";

/** Keep the optional, growing picture panel inside the frozen viewport. */
export function useBrowserPicturePlacement(
  stage: RefObject<HTMLDivElement | null>,
) {
  useLayoutEffect(() => {
    const root = stage.current;
    const pill = root?.querySelector<HTMLElement>(".browser-annotate__pill");
    const panel = root?.querySelector<HTMLElement>(
      ".browser-annotate__details",
    );
    if (!root || !pill || !panel) return;
    const place = () => {
      const bounds = root.getBoundingClientRect();
      const anchor = pill.getBoundingClientRect();
      const above = Math.max(0, anchor.top - bounds.top - 16);
      const below = Math.max(0, bounds.bottom - anchor.bottom - 16);
      const upward = panel.scrollHeight > below && above > below;
      panel.style.boxSizing = "border-box";
      panel.style.width = `${Math.max(0, Math.min(296, bounds.width - 16))}px`;
      panel.style.maxHeight = `${upward ? above : below}px`;
      panel.style.left = `${Math.min(0, bounds.right - anchor.left - panel.offsetWidth - 8)}px`;
      panel.style.top = upward ? "auto" : "calc(100% + 8px)";
      panel.style.bottom = upward ? "calc(100% + 8px)" : "auto";
    };
    const observer = new ResizeObserver(place);
    for (const element of [root, pill, panel]) observer.observe(element);
    place();
    return () => observer.disconnect();
  }, [stage]);
}
