import { useEffect, useMemo, useRef } from "react";

import {
  useAgentArtifactFollowStore,
  type AgentArtifactTarget,
} from "../app/agentArtifactFollowStore";
import {
  agentArtifactOwnerKey,
  type AgentArtifactOwner,
} from "../domain/agentArtifact";
import type { WorkbenchTab } from "../domain/workbench";
import type { HarnessBrowseSession } from "../features/preview/harnessBrowse";
import {
  useHarnessBrowseOwner,
  useHarnessBrowseSession,
} from "../features/preview/useHarnessBrowseSession";

/**
 * The harness browse owner and its Preview. The Preview belongs to the
 * focused Chat only once that Chat owns the harness browse; while the next
 * Chat's projection loads, the previous Chat keeps ownership.
 */
export function useFocusedBrowse(focusedOwner: AgentArtifactOwner | null) {
  const browseOwner = useHarnessBrowseOwner();
  const session = useHarnessBrowseSession(browseOwner);
  const owned = !focusedOwner || browseOwner?.chatId === focusedOwner.chatId;
  return { browseOwner, browseSession: owned ? session : null };
}

export function useAgentArtifactFollowLifecycle({
  availableTargets,
  browseSession,
  browseOwnerChatId = null,
  focusedOwner,
}: {
  availableTargets: AgentArtifactTarget[];
  browseSession: HarnessBrowseSession | null;
  browseOwnerChatId?: string | null;
  focusedOwner: AgentArtifactOwner | null;
}) {
  // While the harness Preview still belongs to the previously focused Chat
  // (the next Chat's projection is loading), it says nothing about this
  // Chat's Preview: observing or reconciling then would misfile a selection.
  const browseSettled =
    browseOwnerChatId === null || browseOwnerChatId === focusedOwner?.chatId;
  useEffect(() => {
    if (!focusedOwner || !browseSession || !browseSettled) return;
    useAgentArtifactFollowStore
      .getState()
      .observe(focusedOwner, { kind: "preview" }, browseSession.revision);
  }, [browseSession, browseSettled, focusedOwner]);

  useEffect(() => {
    if (!focusedOwner || !browseSettled) return;
    useAgentArtifactFollowStore
      .getState()
      .reconcile(focusedOwner, availableTargets);
  }, [availableTargets, browseSettled, focusedOwner]);
}

/** Expands a collapsed resource pane when a tab or Preview revision appears. */
export function useExpandOnNewArtifact({
  artifacts,
  previewOpen,
  browseRevision,
  focusedOwner,
  expanded,
  onExpandedChange,
}: {
  artifacts: readonly WorkbenchTab[];
  previewOpen: boolean;
  browseRevision: number;
  focusedOwner: AgentArtifactOwner | null;
  expanded: boolean;
  onExpandedChange?: (expanded: boolean) => void;
}): void {
  const availableResourceKeys = useRef(new Set<string>());
  const resourceKeys = useMemo(
    () =>
      new Set([
        ...artifacts.map((tab) => tab.id),
        ...(previewOpen
          ? [`preview:${agentArtifactOwnerKey(focusedOwner)}:${browseRevision}`]
          : []),
      ]),
    [artifacts, browseRevision, focusedOwner, previewOpen],
  );

  useEffect(() => {
    const added = [...resourceKeys].some(
      (resourceKey) => !availableResourceKeys.current.has(resourceKey),
    );
    availableResourceKeys.current = resourceKeys;
    if (!expanded && added) onExpandedChange?.(true);
  }, [expanded, onExpandedChange, resourceKeys]);
}
