import { useLayoutEffect, useState, type RefObject } from "react";
import type { TurnProjection } from "../../domain/acp";
import {
  turnElement,
  type TurnListHost,
  type TurnListNavigation,
  type VirtualTurnTarget,
} from "./chatScroll";

/** Upper bound for a smooth scroll; `scrollend` usually settles it sooner. */
const SMOOTH_SCROLL_SETTLE_MS = 1000;

/**
 * Jump and find anchoring over the virtual list's host. It loads with the
 * lazy ChatNavigation chunk; the eager list only keeps a pinned target.
 * The returned ref reads null while no virtual list is mounted, so callers
 * fall back to plain DOM scrolling exactly as before.
 */
export function useTurnListNavigation(
  scrollport: HTMLElement | null,
  // Read only from effects and events, never during render.
  host: RefObject<TurnListHost | null>,
  turns: readonly TurnProjection[],
): RefObject<TurnListNavigation | null> {
  const [controller] = useState(() => createController(() => host.current));
  // A target whose Turn left the projection is dropped.
  useLayoutEffect(() => controller.forget(turns), [turns, controller]);
  useLayoutEffect(() => {
    const port = scrollport;
    if (!port) return;
    const release = (event: Event) => {
      if (
        event.target instanceof Element &&
        event.target.closest(".turn-navigator, .chat-find")
      )
        return;
      if (controller.release())
        port.scrollTo?.({ top: port.scrollTop, behavior: "auto" });
    };
    const surface = port.closest(".chat-surface") ?? port;
    surface.addEventListener("pointerdown", release);
    port.addEventListener("scrollend", controller.finish);
    for (const type of ["wheel", "touchstart", "keydown"])
      port.addEventListener(type, release, { passive: true });
    return () => {
      surface.removeEventListener("pointerdown", release);
      port.removeEventListener("scrollend", controller.finish);
      for (const type of ["wheel", "touchstart", "keydown"])
        port.removeEventListener(type, release);
      controller.clearTimer();
    };
  }, [scrollport, controller]);
  return controller.ref;
}

function createController(currentHost: () => TurnListHost | null) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const clearTimer = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };
  /** Reader input drops the target; true when a smooth scroll was settling. */
  const release = () => {
    const target = currentHost()?.target;
    const settling = target?.current?.settling;
    if (target) target.current = null;
    clearTimer();
    return settling;
  };
  const forget = (turns: readonly TurnProjection[]) => {
    const target = currentHost()?.target;
    if (
      target?.current &&
      !turns.some((turn) => turn.id === target.current!.id)
    ) {
      target.current = null;
      clearTimer();
    }
  };
  const finish = () => {
    clearTimer();
    const host = currentHost();
    if (host?.target.current) {
      host.target.current.settling = false;
      host.refresh();
    }
  };
  const waitForScroll = (behavior: ScrollBehavior) => {
    clearTimer();
    const target = currentHost()?.target.current;
    if (target) target.settling = behavior === "smooth";
    timer =
      behavior === "smooth"
        ? setTimeout(finish, SMOOTH_SCROLL_SETTLE_MS)
        : null;
  };
  const scroll = (port: HTMLElement, top: number, behavior: ScrollBehavior) => {
    waitForScroll(behavior);
    const target = currentHost()?.target.current;
    if (target) target.behavior = "auto";
    if (typeof port.scrollTo === "function") port.scrollTo({ top, behavior });
    else port.scrollTop = top;
  };
  const pin = (
    id: string,
    inset: number,
    prompt: boolean,
    behavior: ScrollBehavior,
  ): VirtualTurnTarget => {
    const target: VirtualTurnTarget = {
      id,
      inset,
      prompt,
      behavior,
      settling: false,
      align(list, scrollport, turns, offsets, readViewport) {
        if (target.settling) return;
        const row = [...list.querySelectorAll<HTMLElement>(".acp-turn")].find(
          (element) => element.dataset.turnId === id,
        );
        if (row) {
          const anchor = prompt
            ? (row.querySelector(".acp-message--user") ?? row)
            : row;
          const difference =
            anchor.getBoundingClientRect().top -
            scrollport.getBoundingClientRect().top -
            inset;
          if (Math.abs(difference) >= 1) {
            scroll(
              scrollport,
              scrollport.scrollTop + difference,
              target.behavior,
            );
            readViewport();
          }
          target.behavior = "auto";
        } else {
          const index = turns.findIndex((turn) => turn.id === id);
          if (index < 0) return;
          const listTop =
            list.getBoundingClientRect().top -
            scrollport.getBoundingClientRect().top +
            scrollport.scrollTop;
          scrollport.scrollTop = listTop + offsets[index]! - inset;
          readViewport();
        }
      },
    };
    return target;
  };
  const navigation: TurnListNavigation = {
    scrollToTurn(id, behavior) {
      const host = currentHost()!;
      const { turns, offsets, list: listRef } = host;
      const index = turns.findIndex((turn) => turn.id === id);
      const list = listRef.current;
      const scrollport = list?.closest<HTMLElement>(".message-list");
      if (index < 0 || !list || !scrollport) return;
      host.clearPins();
      host.target.current = pin(id, 12, true, behavior);
      const row = turnElement(scrollport, id);
      const prompt = row?.querySelector(".acp-message--user") ?? row;
      if (prompt)
        scroll(
          scrollport,
          scrollport.scrollTop +
            prompt.getBoundingClientRect().top -
            scrollport.getBoundingClientRect().top -
            12,
          behavior,
        );
      else
        scrollport.scrollTop = Math.max(
          0,
          list.getBoundingClientRect().top -
            scrollport.getBoundingClientRect().top +
            scrollport.scrollTop +
            offsets[index]! -
            12,
        );
      host.refresh();
    },
    anchorTurn(id, inset, behavior = "auto") {
      const host = currentHost()!;
      host.clearPins();
      host.target.current = pin(id, inset, false, "auto");
      waitForScroll(behavior);
    },
    visibleTurnIds() {
      const { turns, offsets, list: listRef } = currentHost()!;
      const list = listRef.current;
      const port = list?.closest<HTMLElement>(".message-list");
      if (!list || !port) return [];
      const top =
        port.getBoundingClientRect().top - list.getBoundingClientRect().top;
      return turns
        .filter(
          (_, index) =>
            offsets[index + 1]! > top &&
            offsets[index]! < top + port.clientHeight,
        )
        .map((turn) => turn.id);
    },
  };
  const ref: RefObject<TurnListNavigation | null> = {
    get current() {
      return currentHost() ? navigation : null;
    },
  };
  return { clearTimer, finish, forget, release, ref };
}
