import { useLayoutEffect, useRef } from "react";

import {
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../features/preview/harnessBrowse";

/** Recently focused Chats whose harness Preview session survives a switch. */
export const MAX_RETAINED_BROWSE_CHATS = 2;

/**
 * AP-A1 shows one Chat in the centre, so "displayed" alone would drop a
 * Chat's harness Preview the moment another Chat is selected. Retain the two
 * most recently focused Chats instead (an explicit side split keeps its two
 * panes), and keep the current owner while the next Chat's projection loads.
 */
export function useRetainedHarnessBrowse(
  owner: HarnessBrowseOwner | null,
  displayedIds: readonly string[],
  chatIds: readonly string[],
  ownerLoading: boolean,
): void {
  const recent = useRef<string[]>([]);
  useLayoutEffect(() => {
    if (!owner) {
      if (ownerLoading) return;
      recent.current = [];
      setHarnessBrowseOwner(null);
      return;
    }
    recent.current = [
      owner.chatId,
      ...recent.current.filter(
        (id) => id !== owner.chatId && chatIds.includes(id),
      ),
    ].slice(0, MAX_RETAINED_BROWSE_CHATS);
    setHarnessBrowseOwner(
      owner,
      displayedIds.length > 1 ? displayedIds : recent.current,
    );
  }, [owner, displayedIds, chatIds, ownerLoading]);
}
