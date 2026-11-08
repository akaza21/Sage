import { useRef, type KeyboardEvent } from "react";

import { approvalFocusInReach, approvalHold } from "../useApprovalSlot";
// Both composer-slot cards share these styles; each loads on its first use.
import "./permission-prompt.css";

/** A follow-up request this soon after a decision never takes focus. */
const QUEUED_FOCUS_HOLD_MS = 300;

/**
 * Whether a newly arrived card may take focus: only in the focused pane,
 * never right after a decision or a fallback Deny for this request, and
 * never while the person types elsewhere.
 */
export function arrivalFocusAllowed(
  itemId: string,
  card: HTMLElement | null,
  focused: boolean,
  canTakeFocus: () => boolean,
) {
  const held =
    approvalHold.denies.has(itemId) ||
    Date.now() - approvalHold.at < QUEUED_FOCUS_HOLD_MS;
  const pane = card?.closest(".chat-surface") ?? null;
  const inCard = card?.contains(document.activeElement);
  return (
    focused &&
    !held &&
    canTakeFocus() &&
    Boolean(inCard || approvalFocusInReach(pane))
  );
}

/**
 * A focused control of the previous request must not answer this one: park
 * focus on the card itself, where Enter does nothing.
 */
export function parkFocus(card: HTMLElement | null) {
  if (card?.contains(document.activeElement)) card.focus();
}

/**
 * Keys that began outside the card (a held Enter from the composer, a stray
 * keyup) are swallowed, so they never answer a request. A card of buttons
 * swallows every auto-repeat, so a held key answers at most once. With
 * `ownRepeats`, a key pressed down inside the card repeats normally (a held
 * Backspace or arrow in a text field), while a repeat of a key held since
 * before the card arrived is still swallowed.
 */
export function useCardKeyGuard({ ownRepeats = false } = {}) {
  const pressed = useRef(new Set<string>());
  return {
    onKeyDownCapture(event: KeyboardEvent) {
      if (event.repeat && !(ownRepeats && pressed.current.has(event.key))) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      pressed.current.add(event.key);
    },
    onKeyUpCapture(event: KeyboardEvent) {
      // A keyup without its keydown here (Space from the composer) is inert.
      if (!pressed.current.delete(event.key)) {
        event.preventDefault();
        event.stopPropagation();
      }
    },
  };
}
