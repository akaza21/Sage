import {
  useCallback,
  useLayoutEffect,
  useRef,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { useDismissLayer } from "./dismissLayer";
import type { MenuDismissReason } from "./MenuPopover";

/** One typed character, not a shortcut, a dead key or a named key. */
function typedCharacter(event: KeyboardEvent): string | null {
  if (event.ctrlKey || event.metaKey || event.altKey) return null;
  if (event.nativeEvent.isComposing) return null;
  return Array.from(event.key).length === 1 ? event.key : null;
}

/**
 * A menu opened before its body has loaded. Until the body arrives the
 * trigger stands in for it: Escape and Tab on the trigger close it, a press
 * outside closes it, and characters typed on the trigger go to `onType`.
 * `focusMoved` tells the late body whether focus has left where it was when
 * the menu opened; the body then leaves focus alone rather than take it back.
 */
export function useColdOpen({
  dismiss,
  loaded,
  onType,
  open,
  triggerRef,
}: {
  dismiss: (reason: MenuDismissReason) => void;
  loaded: boolean;
  onType?: (text: string) => void;
  open: boolean;
  triggerRef: RefObject<HTMLElement | null>;
}) {
  const cold = open && !loaded;
  const openedFrom = useRef<Element | null>(null);
  // Kept for the whole open, so a body that mounts a commit after the load
  // still reads it.
  useLayoutEffect(() => {
    if (!open) openedFrom.current = null;
    else if (!loaded)
      openedFrom.current ??=
        triggerRef.current?.ownerDocument.activeElement ?? null;
  }, [loaded, open, triggerRef]);
  const focusMoved = useCallback(() => {
    const from = openedFrom.current;
    return from !== null && from.ownerDocument.activeElement !== from;
  }, []);

  useDismissLayer(cold, {
    inside: [triggerRef],
    onOutside: () => dismiss("outside"),
  });

  /** Returns true when it handled the key for the body that is not there. */
  const onKeyDown = (event: KeyboardEvent): boolean => {
    if (!cold) return false;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      dismiss("escape");
      return true;
    }
    if (event.key === "Tab") {
      dismiss("tab");
      return true;
    }
    const typed = onType ? typedCharacter(event) : null;
    if (typed === null) return false;
    // Space would also press the trigger and close the menu again.
    event.preventDefault();
    onType?.(typed);
    return true;
  };
  // A button presses on Space's keyup, so that is held back too.
  const onKeyUp = (event: KeyboardEvent) => {
    if (cold && onType && event.key === " ") event.preventDefault();
  };
  return { focusMoved, onKeyDown, onKeyUp };
}
