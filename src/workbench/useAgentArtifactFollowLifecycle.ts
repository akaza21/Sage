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

/** Pins only tabs the user opened; agent-created tabs never pin. */
export function useSelectOpenedBrowserTab({
  artifacts,
  activeTabId,
  userOpenedBrowserTabId,
  focusedOwner,
}: {
  artifacts: readonly WorkbenchTab[];
  activeTabId: string | null;
  userOpenedBrowserTabId: string | null;
  focusedOwner: AgentArtifactOwner | null;
}): void {
  const knownBrowserTabs = useRef<ReadonlySet<string> | null>(null);
  const browserTabs = useMemo(
    () =>
      new Set(
        artifacts.filter((tab) => tab.kind === "browser").map((tab) => tab.id),
      ),
    [artifacts],
  );
  useEffect(() => {
    const previous = knownBrowserTabs.current;
    knownBrowserTabs.current = browserTabs;
    if (
      !previous ||
      !focusedOwner ||
      !activeTabId ||
      activeTabId !== userOpenedBrowserTabId ||
      previous.has(activeTabId) ||
      !browserTabs.has(activeTabId)
    )
      return;
    useAgentArtifactFollowStore
      .getState()
      .pin(focusedOwner, { kind: "tab", tabId: activeTabId });
  }, [activeTabId, browserTabs, focusedOwner, userOpenedBrowserTabId]);
}

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
