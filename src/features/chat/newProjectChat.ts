import type { AcpChatPage } from "../../domain/acp";
import { isSideChatId } from "../../workbench/sideChatState";
import { composerWorkspaceName } from "./composerChrome";

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

export function newProjectScopeName(
  newProject: boolean,
  page: AcpChatPage | null,
  project: { name: string } | null,
) {
  return newProject && !page?.forkedFrom && !isSideChatId(page?.chatId)
    ? composerWorkspaceName(project, 1024)
    : null;
}
