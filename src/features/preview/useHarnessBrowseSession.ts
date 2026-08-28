import { useSyncExternalStore } from "react";

import {
  type HarnessBrowseSession,
  browseSessionForChat,
  harnessBrowseOwner,
  subscribeBrowseSessions,
} from "./harnessBrowse";

function subscribe(onStoreChange: () => void): () => void {
  return subscribeBrowseSessions(onStoreChange);
}

export function useHarnessBrowseSession(
  chatId: string | null,
): HarnessBrowseSession | null {
  return useSyncExternalStore(
    subscribe,
    () => (chatId ? browseSessionForChat(chatId) : null),
    () => null,
  );
}

export function useHarnessBrowseOwner(): string | null {
  return useSyncExternalStore(subscribe, harnessBrowseOwner, () => null);
}
