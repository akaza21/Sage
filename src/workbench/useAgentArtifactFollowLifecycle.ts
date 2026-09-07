import { useEffect } from "react";

import {
  type AgentArtifactTarget,
  useAgentArtifactFollowStore,
} from "../app/agentArtifactFollowStore";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { HarnessBrowseSession } from "../features/preview/harnessBrowse";

export function useAgentArtifactFollowLifecycle({
  availableTargets,
  browseSession,
  focusedOwner,
}: {
  availableTargets: AgentArtifactTarget[];
  browseSession: HarnessBrowseSession | null;
  focusedOwner: AgentArtifactOwner | null;
}) {
  useEffect(() => {
    if (!focusedOwner || !browseSession) return;
    useAgentArtifactFollowStore
      .getState()
      .observe(focusedOwner, { kind: "preview" }, browseSession.revision);
  }, [browseSession, focusedOwner]);

  useEffect(() => {
    if (!focusedOwner) return;
    useAgentArtifactFollowStore
      .getState()
      .reconcile(focusedOwner, availableTargets);
  }, [availableTargets, focusedOwner]);
}
