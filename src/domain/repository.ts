import type { ResourceRef } from "./resource";

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

export interface RootRepositoryStatus {
  rootId: string;
  state: RepositoryRootState;
  branch: string | null;
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

export type GitFileMutationAction = "stage" | "unstage" | "discard";

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

export type GitProcessState = "terminatedUnreaped" | "potentiallyRunning";

export type GitFileMutationCommandError =
  | { kind: "failed"; message: string }
  | {
      kind: "containment";
      processState: GitProcessState;
      message: string;
    };
