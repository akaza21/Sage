import { useEffect } from "react";

import type { AcpChatPage } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import type { ProjectRootSummary } from "../../../domain/workbench";
import { LazyModuleBoundary } from "../../../ui/LazyModuleBoundary";
import { AcpAnnouncementRegions } from "./AcpAnnouncementRegions";
import { dispatchBrowseFromAcpPage } from "./dispatchBrowse";
import "./acp-activity-cards.css";
import "./acp-chat-activities.css";
import "./acp-chat.css";
import "./acp-child-activity.css";

type AcpChatProjectionProps = {
  page: AcpChatPage;
  active: boolean;
  focused?: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
  hasEarlier?: boolean;
  loadingEarlier?: boolean;
  historyError?: string | null;
  onLoadEarlier?: () => void;
  projectRoots?: readonly Pick<ProjectRootSummary, "id" | "name" | "ordinal">[];
  retentionIncomplete?: boolean;
};

const loadAcpChatPresentation = () =>
  import("./AcpChatPresentation").then(({ AcpChatPresentation }) => ({
    default: AcpChatPresentation,
  }));

export function AcpChatProjection({
  page,
  active,
  focused = true,
  onOpenResource,
  hasEarlier = false,
  loadingEarlier = false,
  historyError = null,
  onLoadEarlier,
  projectRoots = [],
  retentionIncomplete = false,
}: AcpChatProjectionProps) {
  useEffect(() => {
    if (!active || !focused) return;
    dispatchBrowseFromAcpPage(page);
  }, [active, focused, page]);
  if (!active) return null;
  const canonicalRootId =
    projectRoots.length === 1 ? projectRoots[0]?.id : undefined;
  return (
    <section
      className="acp-chat-projection"
      aria-label={`Agent chat ${page.title}`}
      data-projection-revision={page.projectionRevision}
      data-project-id={page.projectId}
      data-chat-id={page.chatId}
      data-current-session-id={page.currentSessionId ?? ""}
      data-expected-resource-root-id={canonicalRootId}
    >
      {focused ? <AcpAnnouncementRegions page={page} /> : null}
      <h2 className="sr-only">{page.title}</h2>
      <LazyModuleBoundary
        label="Chat history"
        load={loadAcpChatPresentation}
        render={(AcpChatPresentation) => (
          <AcpChatPresentation
            page={page}
            onOpenResource={onOpenResource}
            hasEarlier={hasEarlier}
            loadingEarlier={loadingEarlier}
            historyError={historyError}
            onLoadEarlier={onLoadEarlier}
            projectRoots={projectRoots}
            retentionIncomplete={retentionIncomplete}
          />
        )}
      />
    </section>
  );
}
