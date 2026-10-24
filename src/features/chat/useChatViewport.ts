import { useEffect, useRef, useState, type RefObject } from "react";
import type { TurnProjection } from "../../domain/acp";
import type { TurnListNavigation } from "./chatScroll";

export interface ChatViewport {
  /** Turn ids intersecting the scrollport, in transcript order. */
  visibleIds: readonly string[];
  /** Scrollport box relative to its chat surface, read on resize only. */
  top: number;
  height: number;
}

const EMPTY: ChatViewport = { visibleIds: [], top: 0, height: 0 };

function sameIds(left: readonly string[], right: readonly string[]) {
  return (
    left.length === right.length &&
    left.every((id, index) => id === right[index])
  );
}

/**
 * Tracks which Turns are on screen without measuring every Turn on scroll:
 * the virtual list answers from its offsets, and mounted rows report through
 * an IntersectionObserver. Geometry is read only when the scrollport resizes.
 */
export function useChatViewport(
  scrollport: HTMLElement | null,
  turns: readonly TurnProjection[],
  navigationRef: RefObject<TurnListNavigation | null>,
): ChatViewport {
  const [viewport, setViewport] = useState(EMPTY);
  const schedule = useRef<() => void>(() => {});
  useEffect(() => {
    if (!scrollport) return;
    const surface = scrollport.closest(".chat-surface") ?? scrollport;
    const intersecting = new Set<string>();
    let geometry = { top: 0, height: 0 };
    let frame: number | null = null;
    let rescan = true;
    const intersection =
      typeof IntersectionObserver === "undefined"
        ? null
        : new IntersectionObserver(
            (entries) => {
              for (const entry of entries) {
                const id = (entry.target as HTMLElement).dataset.turnId;
                if (!id) continue;
                if (entry.isIntersecting) intersecting.add(id);
                else intersecting.delete(id);
              }
              request();
            },
            { root: scrollport },
          );
    const observed = new Set<Element>();
    const observeRows = (rows: HTMLElement[]) => {
      const current = new Set<Element>(rows);
      for (const row of observed)
        if (!current.has(row)) {
          intersection?.unobserve(row);
          observed.delete(row);
          intersecting.delete((row as HTMLElement).dataset.turnId ?? "");
        }
      for (const row of rows)
        if (!observed.has(row)) {
          observed.add(row);
          intersection?.observe(row);
        }
    };
    const read = () => {
      frame = null;
      const rows = rescan
        ? [...scrollport.querySelectorAll<HTMLElement>(".acp-turn")]
        : null;
      if (rows) {
        rescan = false;
        observeRows(rows);
      }
      let visibleIds = navigationRef.current?.visibleTurnIds();
      // Every supported WebView has IntersectionObserver; without it (jsdom)
      // dashes simply keep their off-screen colour.
      visibleIds ??= [...observed]
        .map((row) => (row as HTMLElement).dataset.turnId!)
        .filter((id) => intersecting.has(id));
      setViewport((value) =>
        sameIds(value.visibleIds, visibleIds) &&
        value.top === geometry.top &&
        value.height === geometry.height
          ? value
          : { visibleIds, ...geometry },
      );
    };
    const request = () => {
      if (frame === null) frame = requestAnimationFrame(read);
    };
    const measure = () => {
      const port = scrollport.getBoundingClientRect();
      const owner = surface.getBoundingClientRect();
      geometry = {
        top: port.top - owner.top,
        height: scrollport.clientHeight || port.height,
      };
    };
    schedule.current = () => {
      rescan = true;
      request();
    };
    const mutation = new MutationObserver(schedule.current);
    mutation.observe(scrollport, { childList: true, subtree: true });
    const resize =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(() => {
            measure();
            request();
          });
    resize?.observe(scrollport);
    // Virtual lists answer from offsets; mounted rows report via observer.
    const onScroll = () => {
      if (navigationRef.current) request();
    };
    scrollport.addEventListener("scroll", onScroll, { passive: true });
    measure();
    read();
    return () => {
      schedule.current = () => {};
      scrollport.removeEventListener("scroll", onScroll);
      mutation.disconnect();
      resize?.disconnect();
      intersection?.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
    };
  }, [scrollport, navigationRef]);
  // New or re-measured Turns move offsets; re-read once per later change
  // (the mount read above already covers the first Turns).
  const seenTurns = useRef(turns);
  useEffect(() => {
    if (seenTurns.current === turns) return;
    seenTurns.current = turns;
    schedule.current();
  }, [turns]);
  return viewport;
}
