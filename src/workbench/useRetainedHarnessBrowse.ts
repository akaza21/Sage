import { useLayoutEffect, useRef } from "react";
import {
  type HarnessBrowseOwner,
  setHarnessBrowseOwner,
} from "../features/preview/harnessBrowse";

/** Recently focused Chats whose harness Preview session survives a switch. */
const MAX_RETAINED_BROWSE_CHATS = 2;

/** Keep the two most recently focused Preview sessions, including an explicit pair. */
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
