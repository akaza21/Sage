import type { RefObject } from "react";
import type { TurnProjection } from "../../domain/acp";

/** A navigation target the virtual list keeps pinned until reader input. */
export interface VirtualTurnTarget {
  id: string;
  inset: number;
  prompt: boolean;
  behavior: ScrollBehavior;
  settling: boolean;
  /**
   * Re-aligns the target after the list renders; installed by the lazy
   * navigator, called from the list's anchoring layout effect.
   */
  align: (
    list: HTMLElement,
    scrollport: HTMLElement,
    turns: readonly TurnProjection[],
    offsets: readonly number[],
    readViewport: () => void,
  ) => void;
}

/**
 * What the eager virtual list exposes. The lazy navigator
 * (useTurnListNavigation) builds TurnListNavigation on top of it, so jump
 * and find anchoring loads with ChatNavigation, not with the transcript.
 */
export interface TurnListHost {
  turns: readonly TurnProjection[];
  offsets: readonly number[];
  list: RefObject<HTMLDivElement | null>;
  target: RefObject<VirtualTurnTarget | null>;
  refresh: () => void;
  clearPins: () => void;
}

export interface TurnListNavigation {
  scrollToTurn: (turnId: string, behavior: ScrollBehavior) => void;
  anchorTurn: (
    turnId: string,
    inset: number,
    behavior?: ScrollBehavior,
  ) => void;
  visibleTurnIds: () => string[];
}

export function chatScrollBehavior(): ScrollBehavior {
  return typeof matchMedia === "function" &&
    matchMedia("(prefers-reduced-motion: reduce)").matches
    ? "auto"
    : "smooth";
}

export function turnElement(scrollport: HTMLElement, turnId: string) {
  return [...scrollport.querySelectorAll<HTMLElement>(".acp-turn")].find(
    (element) => element.dataset.turnId === turnId,
  );
}

export function pauseChatFollow(scrollport: HTMLElement) {
  scrollport.dispatchEvent(new Event("sage-chat-navigation"));
}

export function jumpToChatTurn(
  scrollport: HTMLElement,
  turnId: string,
  navigation: TurnListNavigation | null,
) {
  pauseChatFollow(scrollport);
  if (navigation) return navigation.scrollToTurn(turnId, chatScrollBehavior());
  const turn = turnElement(scrollport, turnId);
  const target = turn?.querySelector(".acp-message--user") ?? turn;
  if (target)
    scrollport.scrollTo({
      top:
        scrollport.scrollTop +
        target.getBoundingClientRect().top -
        scrollport.getBoundingClientRect().top -
        12,
      behavior: chatScrollBehavior(),
    });
}
