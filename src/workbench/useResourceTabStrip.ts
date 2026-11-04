import { useLayoutEffect, type KeyboardEvent, type RefObject } from "react";

/** Automatic activation, one Tab stop, and visibility for a resource tab list. */
export function useResourceTabStrip(ref: RefObject<HTMLElement | null>) {
  useLayoutEffect(() => {
    const strip = ref.current;
    if (!strip) return;
    const observed = new Set<Element>();
    const reveal = () => {
      const tabs = Array.from(
        strip.querySelectorAll<HTMLElement>('[role="tab"]'),
      );
      const selected =
        tabs.find((tab) => tab.getAttribute("aria-selected") === "true") ??
        tabs[0];
      for (const tab of tabs) tab.tabIndex = tab === selected ? 0 : -1;
      const rows = new Set(tabs.map((tab) => tab.closest(".tab") ?? tab));
      for (const row of observed) {
        if (!rows.has(row)) {
          resize?.unobserve(row);
          observed.delete(row);
        }
      }
      for (const row of rows) {
        if (!observed.has(row)) {
          resize?.observe(row);
          observed.add(row);
        }
      }
      selected
        ?.closest(".tab")
        ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    };
    const resize =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(reveal);
    resize?.observe(strip);
    reveal();
    const mutation = new MutationObserver((records) => {
      if (
        records.some(
          (record) =>
            record.type === "attributes" ||
            [...record.addedNodes, ...record.removedNodes].some(
              (node) =>
                node instanceof Element &&
                (node.matches('[role="tab"]') ||
                  node.querySelector('[role="tab"]')),
            ),
        )
      )
        reveal();
    });
    mutation.observe(strip, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["aria-selected"],
    });
    return () => {
      resize?.disconnect();
      mutation.disconnect();
    };
  }, [ref]);

  return (event: KeyboardEvent<HTMLElement>) => {
    if (
      !(event.target instanceof HTMLElement) ||
      event.target.getAttribute("role") !== "tab"
    )
      return;
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const tabs = Array.from(
      ref.current?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [],
    );
    const index = tabs.indexOf(event.target);
    if (index < 0) return;
    event.preventDefault();
    event.stopPropagation();
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? tabs.length - 1
          : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) %
            tabs.length;
    tabs[next]?.focus();
    tabs[next]?.click();
  };
}
