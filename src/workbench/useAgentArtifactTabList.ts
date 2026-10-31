import {
  useId,
  useLayoutEffect,
  useRef,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from "react";

const TAB_SELECTOR = '[role="tab"]';

export function useAgentArtifactTabList(
  root: RefObject<HTMLDivElement | null>,
  ownerKey: string,
  panelId: string,
) {
  const prefix = useId();
  const owner = useRef(ownerKey);
  const pendingClose = useRef<{
    closing: Element;
    next: HTMLButtonElement;
    selected: boolean;
    ownerKey: string;
  } | null>(null);
  useLayoutEffect(() => {
    owner.current = ownerKey;
    pendingClose.current = null;
  }, [ownerKey]);
  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    const sync = () => {
      const closed = pendingClose.current;
      if (closed && !closed.closing.isConnected) {
        pendingClose.current = null;
        if (closed.ownerKey === owner.current && closed.next.isConnected) {
          if (closed.selected) closed.next.click();
          closed.next.focus();
        }
      }
      const tabs = Array.from(
        element.querySelectorAll<HTMLButtonElement>(TAB_SELECTOR),
      );
      const selected =
        tabs.find((tab) => tab.getAttribute("aria-selected") === "true") ??
        tabs[0];
      tabs.forEach((tab, index) => {
        tab.id = `${prefix}-tab-${index}`;
        tab.tabIndex = tab === selected ? 0 : -1;
        tab.setAttribute("aria-controls", panelId);
      });
      const panel = document.getElementById(panelId);
      if (selected) {
        selected.scrollIntoView?.({ block: "nearest", inline: "nearest" });
        panel?.setAttribute("aria-labelledby", selected.id);
      }
    };
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(element, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["aria-selected"],
    });
    return () => observer.disconnect();
  }, [root, ownerKey, panelId, prefix]);
  return {
    onKeyDown: (event: KeyboardEvent<HTMLDivElement>) => {
      const tabs = Array.from(
        root.current?.querySelectorAll<HTMLButtonElement>(TAB_SELECTOR) ?? [],
      );
      const index = tabs.indexOf(event.target as HTMLButtonElement);
      if (
        index < 0 ||
        !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)
      )
        return;
      event.preventDefault();
      const next =
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? tabs.length - 1
            : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) %
              tabs.length;
      tabs[next]?.focus();
      tabs[next]?.click();
    },
    onClickCapture: (event: MouseEvent<HTMLDivElement>) => {
      if (!(event.target instanceof Element)) return;
      const closing = event.target.closest(".tab__close")?.closest(".tab");
      if (!closing) return;
      const tabs = Array.from(
        root.current?.querySelectorAll<HTMLButtonElement>(TAB_SELECTOR) ?? [],
      );
      const index = tabs.findIndex((tab) => closing.contains(tab));
      const next = tabs[index + 1] ?? tabs[index - 1];
      const wasSelected = tabs[index]?.getAttribute("aria-selected") === "true";
      pendingClose.current = next
        ? { closing, next, selected: wasSelected, ownerKey }
        : null;
    },
  };
}
