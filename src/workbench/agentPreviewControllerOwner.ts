import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { HarnessBrowseOwner } from "../features/preview/harnessBrowse";

type FocusedPage = {
  sessions: ReadonlyArray<{ id: string; runtimeEpoch: string | null }>;
};

type FocusedScope = { projectGeneration: string };

export function agentPreviewControllerOwner({
  browseOwner,
  focusedOwner,
  focusedPage,
  focusedScope,
  nativeTransitionEpoch,
  projectGeneration,
}: {
  browseOwner: HarnessBrowseOwner | null;
  focusedOwner: AgentArtifactOwner | null;
  focusedPage: FocusedPage | null;
  focusedScope: FocusedScope | null;
  nativeTransitionEpoch: number;
  projectGeneration: number;
}): HarnessBrowseOwner | null {
  const session = focusedPage?.sessions.find(
    (candidate) => candidate.id === browseOwner?.sessionId,
  );
  return focusedOwner &&
    browseOwner &&
    browseOwner.projectId === focusedOwner.projectId &&
    browseOwner.chatId === focusedOwner.chatId &&
    browseOwner.sessionId === focusedOwner.sessionId &&
    session?.runtimeEpoch === browseOwner.runtimeEpoch &&
    focusedScope?.projectGeneration === browseOwner.projectGeneration &&
    browseOwner.projectGeneration === String(projectGeneration) &&
    browseOwner.projectGeneration === String(nativeTransitionEpoch) &&
    browseOwner.transitionEpoch === nativeTransitionEpoch
    ? browseOwner
    : null;
}
