import type { FileDiff } from "./editor";
import type { ResourceRef } from "./resource";

export interface RepositoryRef {
  rootId: string;
  relativePath: string;
}

export interface DiscoverProjectRepositoriesRequest {
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
}

export interface DiscoverProjectRepositoriesResult {
  projectId: string;
  transitionEpoch: number;
  refs: RepositoryRef[];
  truncated: boolean;
}

export interface NestedRepositoryStatusRequest {
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  ref: RepositoryRef;
}

export interface NestedRepositoryStatusResult {
  projectId: string;
  transitionEpoch: number;
  ref: RepositoryRef;
  status: RootRepositoryStatus;
}

export interface NestedRepositoryFileDiffRequest {
  projectId: string;
  projectGeneration: number;
  expectedTransitionEpoch: number;
  ref: RepositoryRef;
  resource: ResourceRef;
}

export interface NestedRepositoryFileDiffResult {
  projectId: string;
  transitionEpoch: number;
  diff: FileDiff;
}

export type GitFileState =
  | "unchanged"
  | "modified"
  | "typeChanged"
  | "added"
  | "deleted"
  | "renamed"
  | "copied"
  | "conflicted"
  | "untracked"
  | "unknown";

export interface FileChange {
  resource: ResourceRef;
  originalResource?: ResourceRef | null;
  indexStatus: GitFileState;
  worktreeStatus: GitFileState;
  additions: number | null;
  deletions: number | null;
}

export type RepositoryRootState =
  "repository" | "notRepository" | "unavailable";

export interface GitRecentCommit {
  sha: string;
  subject: string;
}

export interface RootRepositoryStatus {
  rootId: string;
  state: RepositoryRootState;
  branch: string | null;
  /** Native-proven tracking ref for the current branch; absent for old snapshots. */
  upstream?: string | null;
  ahead?: number;
  behind?: number;
  branches?: string[];
  branchesTruncated?: boolean;
  recentCommits?: GitRecentCommit[];
  recentCommitsTruncated?: boolean;
  remotes?: string[];
  remotesTruncated?: boolean;
  mergeInProgress?: boolean;
  rebaseInProgress?: boolean;
  cherryPickInProgress?: boolean;
  revertInProgress?: boolean;
  changes: FileChange[];
  truncated: boolean;
  elapsedMs: number;
  message: string | null;
}

export interface ProjectRepositoryStatus {
  roots: RootRepositoryStatus[];
  truncated: boolean;
  elapsedMs: number;
}

export type GitFileMutationAction =
  "stage" | "unstage" | "discard" | "keepOurs" | "keepTheirs";

export interface GitFileMutationRequest {
  projectId: string;
  resource: ResourceRef;
  action: GitFileMutationAction;
  discardConfirmed: boolean;
}

export type GitFileMutationErrorKind = "effectUnknown";

export interface GitFileMutationError {
  kind: GitFileMutationErrorKind;
  message: string;
}

export interface GitFileMutationResult {
  projectId: string;
  transitionEpoch: number;
  repositories: ProjectRepositoryStatus;
  error: GitFileMutationError | null;
}

export interface GitRepositoryCommitRequest {
  projectId: string;
  rootId: string;
  message: string;
}

export interface GitRepositoryCommitResult {
  projectId: string;
  rootId: string;
  transitionEpoch: number;
  repositories: ProjectRepositoryStatus;
  error: GitFileMutationError | null;
}

export interface GitRepositoryCheckoutRequest {
  projectId: string;
  rootId: string;
  branch: string;
}

export interface GitRepositoryBranchRenameRequest {
  projectId: string;
  rootId: string;
  from: string;
  to: string;
}

export interface GitRepositoryCheckoutResult {
  projectId: string;
  rootId: string;
  transitionEpoch: number;
  repositories: ProjectRepositoryStatus;
  error: GitFileMutationError | null;
}

export interface GitRepositoryRebaseContinueRequest {
  projectId: string;
  rootId: string;
}

export type GitRepositoryRebaseContinueResult = GitRepositoryCheckoutResult;

export interface GitRepositoryCherryPickContinueRequest {
  projectId: string;
  rootId: string;
}

export type GitRepositoryCherryPickContinueResult = GitRepositoryCheckoutResult;

export interface GitRepositoryRevertContinueRequest {
  projectId: string;
  rootId: string;
}

export interface GitRepositorySequenceAbortRequest {
  projectId: string;
  rootId: string;
}

export type GitRepositoryRevertContinueResult = GitRepositoryCheckoutResult;

export interface GitRepositoryRemoteTransportRequest {
  projectId: string;
  rootId: string;
  remote: string;
}

export type GitRepositoryRemoteTransportResult = GitRepositoryCheckoutResult;

export interface GitRepositoryPullRequest {
  projectId: string;
  rootId: string;
}

export type GitRepositoryPullResult = GitRepositoryCheckoutResult;

export type GitProcessState = "terminatedUnreaped" | "potentiallyRunning";

export type GitFileMutationCommandError =
  | { kind: "failed"; message: string }
  | {
      kind: "containment";
      processState: GitProcessState;
      message: string;
    };
