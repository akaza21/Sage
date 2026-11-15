import { useEffect, useRef, type RefObject } from "react";
import { chatMatchRanges, publishChatHighlights } from "./chatFindHighlights";
import { type ChatMatch } from "./chatFindModel";
import {
  chatScrollBehavior,
  pauseChatFollow,
  turnElement,
  type TurnListNavigation,
} from "./chatScroll";

function centreRange(
  scrollport: HTMLElement,
  navigation: TurnListNavigation | null,
  range: Range,
  turnId: string,
) {
  const bounds = range.getBoundingClientRect();
  const viewport = scrollport.getBoundingClientRect();
  const delta =
    bounds.top + bounds.height / 2 - viewport.top - scrollport.clientHeight / 2;
  const behavior = chatScrollBehavior();
  pauseChatFollow(scrollport);
  const turn = turnElement(scrollport, turnId);
  // Hold the Turn where the scroll lands, so virtual rows that are measured
  // on the way do not push the match off-centre.
  if (turn)
    navigation?.anchorTurn(
      turnId,
      turn.getBoundingClientRect().top - viewport.top - delta,
      behavior,
    );
  if (typeof scrollport.scrollTo === "function")
    scrollport.scrollTo({ top: scrollport.scrollTop + delta, behavior });
  else scrollport.scrollTop += delta;
}

export function useChatFindHighlights({
  scrollport,
  navigationRef,
  matches,
  current,
  revision,
}: {
  scrollport: HTMLElement;
  navigationRef: RefObject<TurnListNavigation | null>;
  matches: readonly ChatMatch[];
  current: number;
  /** Bump for every navigation step so the same match can be recentered. */
  revision: number;
}) {
  const owner = useRef(Symbol("chat-find"));
  const revealed = useRef("");
  const match = matches[current];
  const selection = match ? `${match.key}\u0000${revision}` : "";
  useEffect(() => {
    if (!selection) revealed.current = "";
    if (
      match &&
      revealed.current !== selection &&
      !turnElement(scrollport, match.turnId)
    ) {
      pauseChatFollow(scrollport);
      navigationRef.current?.scrollToTurn(match.turnId, "auto");
    }
    let frame: number | null = null;
    const paint = () => {
      frame = null;
      const ranges = chatMatchRanges(scrollport, matches);
      const focus = match ? ranges.get(match.key) : undefined;
      if (focus) ranges.delete(match!.key);
      publishChatHighlights(owner.current, {
        other: [...ranges.values()],
        current: focus ? [focus] : [],
      });
      if (focus && revealed.current !== selection) {
        revealed.current = selection;
        centreRange(scrollport, navigationRef.current, focus, match!.turnId);
      }
    };
    const schedule = () => {
      if (frame === null) frame = requestAnimationFrame(paint);
    };
    // Text edits keep live Ranges valid; only mounting changes the set.
    const observer = new MutationObserver(schedule);
    observer.observe(scrollport, { childList: true, subtree: true });
    paint();
    return () => {
      observer.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
    };
  }, [scrollport, navigationRef, matches, match, selection]);
  useEffect(() => {
    const identity = owner.current;
    return () => publishChatHighlights(identity, null);
  }, []);
}
