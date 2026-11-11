/** The first tooltip waits this long, like a native title but shorter. */
export const TOOLTIP_DELAY_MS = 600;
/** After one hides, the next shows at once for this long (moving along a toolbar). */
export const TOOLTIP_SKIP_DELAY_MS = 300;
/** Leaving the control waits this long, so the pointer can reach the tooltip. */
export const TOOLTIP_HIDE_GRACE_MS = 100;
const GAP = 6;
const EDGE = 8;

/**
 * Controls that ask for the tooltip: `data-tooltip`, or an `IconButton`
 * without a native title, which shows its accessible name.
 */
const ANCHOR = "[data-tooltip], .icon-button[aria-label]:not([title])";

function anchorOf(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>(ANCHOR) : null;
}

function tooltipText(anchor: HTMLElement) {
  return anchor.dataset.tooltip ?? anchor.getAttribute("aria-label");
}

function focusVisible(element: Element) {
  try {
    return element.matches(":focus-visible");
  } catch {
    // Engines without :focus-visible treat every focus as visible.
    return true;
  }
}

/** Above the control and centred; below it when there is no room above. */
function place(node: HTMLElement, rect: DOMRect) {
  const { width, height } = node.getBoundingClientRect();
  const above = rect.top - GAP - height;
  const top = above >= EDGE ? above : rect.bottom + GAP;
  const left = Math.max(
    EDGE,
    Math.min(
      window.innerWidth - EDGE - width,
      rect.left + rect.width / 2 - width / 2,
    ),
  );
  node.style.left = `${Math.round(left)}px`;
  node.style.top = `${Math.round(top)}px`;
}

/**
 * Shows the `data-tooltip` text of the control under the pointer, or of the
 * control focused from the keyboard, in one shared node. The control keeps
 * its accessible name, so the node is hidden from assistive technology.
 * Returns an uninstall.
 */
export function installTooltipLayer(): () => void {
  const node = document.createElement("div");
  node.className = "tooltip";
  node.setAttribute("role", "tooltip");
  node.setAttribute("aria-hidden", "true");
  node.hidden = true;
  document.body.append(node);

  let anchor: HTMLElement | null = null;
  let shown = false;
  let timer: number | undefined;
  let warmUntil = 0;
  // Follows the shown control: moves with it, and hides once it is removed.
  const resize =
    typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver(() => {
          if (!anchor?.isConnected) hide();
          else if (shown) place(node, anchor.getBoundingClientRect());
        });

  function hide() {
    window.clearTimeout(timer);
    if (shown) warmUntil = performance.now() + TOOLTIP_SKIP_DELAY_MS;
    resize?.disconnect();
    anchor = null;
    shown = false;
    node.hidden = true;
  }

  function show() {
    const text = anchor?.isConnected ? tooltipText(anchor) : "";
    if (!anchor || !text) return hide();
    node.textContent = text;
    node.hidden = false;
    shown = true;
    place(node, anchor.getBoundingClientRect());
    resize?.observe(anchor);
  }

  function open(next: HTMLElement, immediate: boolean) {
    if (next === anchor) {
      // Back on the control (or on a child of it): keep it, or show it now.
      if (shown || immediate) window.clearTimeout(timer);
      if (immediate && !shown) show();
      return;
    }
    const warm = shown || performance.now() < warmUntil;
    hide();
    anchor = next;
    if (immediate || warm) show();
    else timer = window.setTimeout(show, TOOLTIP_DELAY_MS);
  }

  const onPointerOver = (event: PointerEvent) => {
    if (event.pointerType === "touch") return;
    const next = anchorOf(event.target);
    if (next) open(next, false);
    else if (node.contains(event.target as Node)) window.clearTimeout(timer);
    else if (shown) {
      window.clearTimeout(timer);
      timer = window.setTimeout(hide, TOOLTIP_HIDE_GRACE_MS);
    } else hide();
  };
  const onFocusIn = (event: FocusEvent) => {
    const next = anchorOf(event.target);
    if (next && focusVisible(next)) open(next, true);
    else hide();
  };
  const onFocusOut = (event: FocusEvent) => {
    if (anchor && anchor.contains(event.target as Node)) hide();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    // Hides without taking the key: the same Escape still closes the menu
    // or dialog the control sits in.
    if (event.key === "Escape") hide();
  };

  const onScroll = (event: Event) => {
    // Only a scroll that moves the control; a transcript streaming elsewhere
    // must not hide the composer's tooltips.
    const scroller = event.target;
    if (
      anchor &&
      (scroller === document ||
        (scroller instanceof Node && scroller.contains(anchor)))
    )
      hide();
  };

  const listeners: [string, EventListener][] = [
    ["pointerover", onPointerOver as EventListener],
    ["focusin", onFocusIn as EventListener],
    ["focusout", onFocusOut as EventListener],
    ["keydown", onKeyDown as EventListener],
    ["pointerdown", hide],
    ["scroll", onScroll as EventListener],
  ];
  for (const [type, listener] of listeners)
    document.addEventListener(type, listener, true);
  window.addEventListener("blur", hide);

  return () => {
    hide();
    for (const [type, listener] of listeners)
      document.removeEventListener(type, listener, true);
    window.removeEventListener("blur", hide);
    node.remove();
  };
}
