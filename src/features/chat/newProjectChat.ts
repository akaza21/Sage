import type { AcpChatPage } from "../../domain/acp";

export function isNewProjectChat({
  useNativeAcp,
  page,
  project,
  projectTransitioning,
  loadingEarlier,
  historyError,
  hasEarlier,
  retentionIncomplete,
}: {
  useNativeAcp: boolean;
  page: AcpChatPage | null;
  project: { id: string } | null;
  projectTransitioning: boolean;
  loadingEarlier: boolean;
  historyError: string | null;
  hasEarlier: boolean;
  retentionIncomplete: boolean;
}) {
  return Boolean(
    useNativeAcp &&
    project &&
    page?.projectId === project.id &&
    page.turns.length === 0 &&
    !projectTransitioning &&
    !loadingEarlier &&
    !historyError &&
    !hasEarlier &&
    page.nextCursor === null &&
    !retentionIncomplete,
  );
}
