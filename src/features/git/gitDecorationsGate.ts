import { useEffect, useState } from "react";

import { isConflictedChange } from "../../domain/gitMutation";
import type {
  FileChange,
  GitFileState,
  ProjectRepositoryStatus,
} from "../../domain/repository";
import type { ResourceRef } from "../../domain/workbench";

export type MarkedGitState = Exclude<GitFileState, "unchanged" | "unknown">;

export function markedGitState(change: FileChange): MarkedGitState | null {
  if (isConflictedChange(change)) return "conflicted";
  // The working tree's state, else the staged one: when the working tree is
  // clean, or "unknown" in the post-commit index-only status.
  const state =
    change.worktreeStatus === "unchanged" || change.worktreeStatus === "unknown"
      ? change.indexStatus
      : change.worktreeStatus;
  return state === "unchanged" || state === "unknown" || state === "conflicted"
    ? null
    : state;
}

export function resourceHasGitMark(
  repositories: ProjectRepositoryStatus,
  resource: ResourceRef,
): boolean {
  return (
    repositories.roots
      .find(
        (root) =>
          root.rootId === resource.rootId && root.state === "repository",
      )
      ?.changes.some(
        (change) =>
          change.resource.relativePath === resource.relativePath &&
          markedGitState(change) !== null,
      ) ?? false
  );
}

import type * as GitDecorationsUi from "./gitDecorationsUi";

export type GitDecorationsUiModule = typeof GitDecorationsUi;

let loaded: GitDecorationsUiModule | undefined;

export function useGitDecorationsUi(
  wanted: boolean,
): GitDecorationsUiModule | undefined {
  const [module, setModule] = useState(loaded);
  // A load that finished while not wanted is ready the moment it is again.
  const ready = module ?? loaded;
  useEffect(() => {
    if (!wanted || ready) return;
    let live = true;
    import("./gitDecorationsUi").then(
      (next) => {
        loaded = next;
        if (live) setModule(next);
      },
      (error: unknown) => console.warn("Git decorations could not load", error),
    );
    return () => {
      live = false;
    };
  }, [ready, wanted]);
  return wanted ? ready : undefined;
}
