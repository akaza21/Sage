import type { RefObject } from "react";
import type { TurnProjection } from "../../domain/acp";

export interface VirtualTurnTarget {
  id: string;
  inset: number;
  prompt: boolean;
  behavior: ScrollBehavior;
  settling: boolean;
  align: (
    list: HTMLElement,
    scrollport: HTMLElement,
    turns: readonly TurnProjection[],
    offsets: readonly number[],
    readViewport: () => void,
  ) => void;
}

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
