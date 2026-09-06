import { useSyncExternalStore } from "react";

import {
  type HarnessBrowseSession,
  type HarnessBrowseOwner,
  browseSessionForOwner,
  harnessBrowseOwner,
  subscribeBrowseSessions,
} from "./harnessBrowse";

function subscribe(onStoreChange: () => void): () => void {
  return subscribeBrowseSessions(onStoreChange);
}

export function useHarnessBrowseSession(
  owner: HarnessBrowseOwner | null,
): HarnessBrowseSession | null {
  return useSyncExternalStore(
    subscribe,
    () => (owner ? browseSessionForOwner(owner) : null),
    () => null,
  );
}

export function useHarnessBrowseOwner(): HarnessBrowseOwner | null {
  return useSyncExternalStore(subscribe, harnessBrowseOwner, () => null);
}
