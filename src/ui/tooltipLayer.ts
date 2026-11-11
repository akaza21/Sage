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
  // Measure at the origin: a fixed box left near the right edge by the last
  // tooltip would wrap into the space that remains there.
  node.style.left = "0px";
  node.style.top = "0px";
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
 * Shows the tooltip text of the control under the pointer, or of the control
 * focused from the keyboard, in one shared node. The control keeps its
 * accessible name, so the node is hidden from assistive technology.
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
  // A shown control that is removed, hidden or resized hides the tooltip.
  const resize =
    typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver(() => {
          if (anchor && !anchor.getClientRects().length) hide();
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
    if (!anchor || !text || !anchor.getClientRects().length) return hide();
    // Inside a modal <dialog> only the dialog's own top layer is visible.
    const host = anchor.closest("dialog") ?? document.body;
    if (node.parentElement !== host) host.append(node);
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

  /** Hides after a short grace, unless the pointer comes back first. */
  function leave() {
    window.clearTimeout(timer);
    if (shown) timer = window.setTimeout(hide, TOOLTIP_HIDE_GRACE_MS);
    else hide();
  }

  const controller = new AbortController();
  const options = { capture: true, signal: controller.signal };
  document.addEventListener(
    "pointerover",
    (event) => {
      if (event.pointerType === "touch") return;
      const next = anchorOf(event.target);
      if (next) open(next, false);
      else if (node.contains(event.target as Node)) window.clearTimeout(timer);
    },
    options,
  );
  // Leaving the control or the tooltip for anything else, including another
  // window, a native page, or a disabled control that gets no pointer events.
  document.addEventListener(
    "pointerout",
    (event) => {
      const to = event.relatedTarget as Node | null;
      const from = event.target as Node;
      if (
        anchor &&
        (anchor.contains(from) || node.contains(from)) &&
        !(to && (anchor.contains(to) || node.contains(to)))
      )
        leave();
    },
    options,
  );
  document.addEventListener(
    "focusin",
    (event) => {
      const next = anchorOf(event.target);
      if (next && focusVisible(next)) open(next, true);
    },
    options,
  );
  document.addEventListener(
    "focusout",
    (event) => {
      if (anchor?.contains(event.target as Node)) hide();
    },
    options,
  );
  // Any key hides it, as a native title does: a press may relabel or move
  // the control. The key is never taken, so Escape still closes the menu or
  // dialog around the control.
  document.addEventListener("keydown", hide, options);
  document.addEventListener("pointerdown", hide, options);
  document.addEventListener(
    "scroll",
    (event) => {
      // Only a scroll that moves the control; a transcript streaming
      // elsewhere must not hide the composer's tooltips.
      const scroller = event.target;
      if (
        anchor &&
        (scroller === document ||
          (scroller instanceof Node && scroller.contains(anchor)))
      )
        hide();
    },
    options,
  );
  window.addEventListener("blur", hide, { signal: controller.signal });

  return () => {
    hide();
    controller.abort();
    node.remove();
  };
}
