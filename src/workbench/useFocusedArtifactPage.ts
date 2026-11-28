import { useMemo } from "react";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import {
  latestSubagentForPage,
  ownerFromFocusedPage,
  subagentPaneForPage,
  useSubagentPaneStore,
  validFocusedSubagentPage,
} from "../app/subagentPaneStore";
import type { AgentArtifactOwner } from "../domain/agentArtifact";

/** The focused Chat's page, owner and subagent target for the right pane. */
export function useFocusedArtifactPage() {
  const subagentOpen = useSubagentPaneStore((state) => state.open);
  const subagentSelectedState = useSubagentPaneStore((state) => state.selected);
  const project = useWorkbenchStore((state) => state.project);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const focusedChatId = usePinnedAgentChatOwner();
  const focusedProjectionStore = acpProjectionStoreFor(focusedChatId);
  const focusedPage = useStore(focusedProjectionStore, (state) => state.page);
  const focusedScope = useStore(focusedProjectionStore, (state) => state.scope);
  const focusedPageValid = validFocusedSubagentPage(
    focusedPage,
    focusedScope,
    project?.id ?? null,
    focusedChatId,
    projectGeneration,
    projectTransitioning,
  );
  const focusedOwner = useMemo<AgentArtifactOwner | null>(
    () => ownerFromFocusedPage(focusedPage, focusedPageValid),
    [focusedPage, focusedPageValid],
  );
  const selectedSubagent = subagentPaneForPage(
    subagentOpen,
    focusedPageValid ? focusedPage : null,
  );
  const subagentTarget =
    selectedSubagent ??
    latestSubagentForPage(focusedPageValid ? focusedPage : null);
  const subagentAvailable = subagentTarget !== null;
  return {
    subagentOpen,
    subagentSelectedState,
    project,
    projectGeneration,
    nativeTransitionEpoch,
    projectTransitioning,
    focusedChatId,
    focusedPage,
    focusedScope,
    focusedPageValid,
    focusedOwner,
    selectedSubagent,
    subagentTarget,
    subagentAvailable,
  };
}
