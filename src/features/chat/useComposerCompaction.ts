import { useLayoutEffect, useState, type RefObject } from "react";

export function useComposerCompaction(trigger: RefObject<HTMLElement | null>) {
  const [width, setWidth] = useState(Infinity);
  useLayoutEffect(() => {
    const composer = trigger.current?.closest(".composer");
    if (!composer || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width > 0) setWidth(entry.contentRect.width);
    });
    observer.observe(composer);
    return () => observer.disconnect();
  }, [trigger]);
  return { effort: width <= 320, project: width <= 280 };
}
