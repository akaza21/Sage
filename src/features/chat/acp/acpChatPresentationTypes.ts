import type { TurnWorkspaceChange } from "./turnWorkspaceChanges";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import type { ProjectRootSummary } from "../../../domain/workbench";

export type AcpChatPresentationProps = {
  page: AcpChatPage;
  focused?: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
  hasEarlier: boolean;
  loadingEarlier: boolean;
  historyError: string | null;
  onLoadEarlier?: () => void;
  projectRoots: readonly Pick<ProjectRootSummary, "id" | "name" | "ordinal">[];
  retentionIncomplete: boolean;
  onRetry?: () => void;
  editSummaryContainer?: HTMLElement | null;
};

export interface AcpChatTurnProps {
  page: AcpChatPage;
  turn: TurnProjection;
  /** A fork's read-only parent Turn: never interactive or quotable. */
  inherited?: boolean;
  /** Earlier/inherited answers retain Copy (s2/f0066). */
  answerActions?: boolean;
  rootLabels?: Readonly<Record<string, string>>;
  workspaceChanges?: readonly TurnWorkspaceChange[];
  workspaceIncomplete?: boolean;
  onOpenResource?: AcpChatPresentationProps["onOpenResource"];
  onRetry?: () => void;
}
