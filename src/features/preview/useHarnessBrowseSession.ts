import { useSyncExternalStore } from "react";

import {
  type HarnessBrowseSession,
  type HarnessBrowseOwner,
  browseSessionForOwner,
  harnessBrowseOwner,
  subscribeBrowseSessions,
} from "./harnessBrowse";

export function useHarnessBrowseSession(
  owner: HarnessBrowseOwner | null,
): HarnessBrowseSession | null {
  return useSyncExternalStore(
    subscribeBrowseSessions,
    () => (owner ? browseSessionForOwner(owner) : null),
    () => null,
  );
}

export function useHarnessBrowseOwner(): HarnessBrowseOwner | null {
  return useSyncExternalStore(
    subscribeBrowseSessions,
    harnessBrowseOwner,
    () => null,
  );
}
