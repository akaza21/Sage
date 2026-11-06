import type { AcpChatPage } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import type { ProjectRootSummary } from "../../../domain/workbench";

export type AcpChatPresentationProps = {
  page: AcpChatPage;
  onOpenResource?: AgentArtifactOpenHandler;
  hasEarlier: boolean;
  loadingEarlier: boolean;
  historyError: string | null;
  onLoadEarlier?: () => void;
  projectRoots: readonly Pick<ProjectRootSummary, "id" | "name" | "ordinal">[];
  retentionIncomplete: boolean;
  onRetry?: () => void;
  /** Omit for inline files; null reserves a dock that is sleeping or mounting. */
  editSummaryContainer?: HTMLElement | null;
};
