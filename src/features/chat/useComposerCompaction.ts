import { useLayoutEffect, useState, type RefObject } from "react";

/** Share composer measurements between compaction and whole-name fitting. */
export function useComposerCompaction(
  trigger: RefObject<HTMLElement | null>,
  harness: RefObject<HTMLElement | null>,
) {
  const [width, setWidth] = useState(Infinity);
  useLayoutEffect(() => {
    const composer = trigger.current?.closest(".composer");
    if (!composer || typeof ResizeObserver === "undefined") return;
    const host = harness.current;
    const fit = () => {
      if (!host) return;
      host.removeAttribute("data-name-hidden");
      const name = host.querySelector(".searchable-picker__trigger-label");
      if (name && name.scrollWidth > name.clientWidth)
        host.setAttribute("data-name-hidden", "");
    };
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width > 0) setWidth(entry.contentRect.width);
      fit();
    });
    observer.observe(composer);
    const text = new MutationObserver(fit);
    if (host?.parentElement)
      text.observe(host.parentElement, {
        subtree: true,
        childList: true,
        characterData: true,
      });
    fit();
    return () => {
      observer.disconnect();
      text.disconnect();
    };
  }, [trigger, harness]);
  return { effort: width <= 320, project: width <= 280 };
}
