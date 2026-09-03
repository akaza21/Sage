import { useSyncExternalStore } from "react";

import {
  type HarnessBrowseSession,
  browseSessionForOwner,
  harnessBrowseOwner,
  subscribeBrowseSessions,
} from "./harnessBrowse";
import type { AgentArtifactOwner } from "../../domain/agentArtifact";

function subscribe(onStoreChange: () => void): () => void {
  return subscribeBrowseSessions(onStoreChange);
}

export function useHarnessBrowseSession(
  owner: AgentArtifactOwner | null,
): HarnessBrowseSession | null {
  return useSyncExternalStore(
    subscribe,
    () => (owner ? browseSessionForOwner(owner) : null),
    () => null,
  );
}

export function useHarnessBrowseOwner(): AgentArtifactOwner | null {
  return useSyncExternalStore(subscribe, harnessBrowseOwner, () => null);
}
