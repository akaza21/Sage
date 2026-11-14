import { useRef, type KeyboardEvent } from "react";

import { approvalFocusInReach, approvalHold } from "../useApprovalSlot";
import "./permission-prompt.css";

/** A follow-up request this soon after a decision never takes focus. */
const QUEUED_FOCUS_HOLD_MS = 300;

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

const cardsByRequest = new Map<string, string>();

export function logCardArrival(
  item: { id: string; payload: { requestId: string } },
  card: HTMLElement | null,
) {
  const { requestId } = item.payload;
  const previous = cardsByRequest.get(requestId);
  const arrival =
    previous === undefined
      ? "new request"
      : previous === item.id
        ? "remount of the same request"
        : `same request re-projected (was item ${previous})`;
  cardsByRequest.delete(requestId);
  cardsByRequest.set(requestId, item.id);
  if (cardsByRequest.size > 32)
    cardsByRequest.delete(cardsByRequest.keys().next().value!);
  const live = [
    ...(card?.ownerDocument ?? document).querySelectorAll<HTMLElement>(
      "section.acp-permission-prompt[data-acp-item-id]",
    ),
  ].map((section) => section.dataset.acpItemId);
  const detail = { itemId: item.id, requestId, arrival, live };
  if (live.length > 1)
    console.warn("approval card: more than one card", detail);
  else console.info(`approval card: ${arrival}`, detail);
}

// A focused control of the previous request must not answer this one.
export function parkFocus(card: HTMLElement | null) {
  if (card?.contains(document.activeElement)) card.focus();
}

// Keys that began before the card arrived (a held Enter from the composer, a
// stray keyup) must never answer a request.
// Button-only approval cards suppress every repeat; question cards permit
// internally begun repeats so editing keys work.
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
