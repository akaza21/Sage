import { useId } from "react";

import { useWorkbenchStore } from "../app/store";
import { exclusiveGitSequence } from "../domain/gitCommit";
import type { RootRepositoryStatus } from "../domain/workbench";
import { repositoryPrimaryAction } from "../features/git/primaryAction";
import { RepositoryCommitControl } from "./RepositoryCommitControl";

interface SourceRepositoryControlProps {
  repository: RootRepositoryStatus;
  rootLabel: string;
  controlsDisabled: boolean;
  writable: boolean;
}

function branchLabel(repository: RootRepositoryStatus): string {
  return (
    repository.branch ??
    (repository.state === "notRepository" ? "Not a repository" : "Unavailable")
  );
}

function repositoryAccessibleName(
  repository: RootRepositoryStatus,
  rootLabel: string,
): string {
  const branch = branchLabel(repository);
  if (repository.state !== "repository") {
    return `${rootLabel} repository, ${branch.toLowerCase()}`;
  }
  const upstream = repository.upstream
    ? `upstream ${repository.upstream}`
    : "no upstream";
  return `${rootLabel} repository, branch ${branch}, ${upstream}, ${repository.ahead ?? 0} ahead, ${repository.behind ?? 0} behind`;
}

export function SourceRepositoryControl({
  repository,
  rootLabel,
  controlsDisabled,
  writable,
}: SourceRepositoryControlProps) {
  const reasonId = useId();
  const pullRepository = useWorkbenchStore((state) => state.pullRepository);
  const pushRemote = useWorkbenchStore((state) => state.pushRemote);
  const branch = branchLabel(repository);
  const action = writable ? repositoryPrimaryAction(repository) : null;
  const sequence = exclusiveGitSequence(repository);
  const showCommitControl =
    writable &&
    repository.state === "repository" &&
    (sequence !== null || action === null || action.kind === "commit");
  const transportAction = action && action.kind !== "commit" ? action : null;
  const trackingTitle = repository.upstream
    ? `${repository.upstream} · ${repository.ahead ?? 0} ahead, ${repository.behind ?? 0} behind`
    : `No upstream · ${repository.ahead ?? 0} ahead, ${repository.behind ?? 0} behind`;

  return (
    <>
      <div
        className="source-root__summary"
        role="group"
        aria-label={repositoryAccessibleName(repository, rootLabel)}
      >
        <span className="source-root__name" title={rootLabel}>
          {rootLabel}
        </span>
        <span className="source-root__branch" title={`Branch ${branch}`}>
          {branch}
        </span>
        {repository.state === "repository" ? (
          <span className="source-root__identity" title={trackingTitle}>
            <span className="source-root__upstream">
              {repository.upstream ?? "No upstream"}
            </span>
            <span
              className="source-root__divergence"
              aria-label={`${repository.ahead ?? 0} ahead, ${repository.behind ?? 0} behind`}
            >
              ↑{repository.ahead ?? 0} ↓{repository.behind ?? 0}
            </span>
          </span>
        ) : null}
        {transportAction ? (
          <button
            type="button"
            className="source-root__primary"
            disabled={controlsDisabled || transportAction.reason !== null}
            aria-describedby={
              transportAction.reason !== null ? reasonId : undefined
            }
            title={
              transportAction.reason ??
              (transportAction.kind === "pull"
                ? "Pull the current upstream"
                : `${transportAction.label} using ${transportAction.remote}`)
            }
            onClick={() => {
              if (transportAction.kind === "pull") {
                void pullRepository(repository.rootId);
              } else if (
                transportAction.kind === "push" ||
                transportAction.kind === "publish"
              ) {
                void pushRemote(repository.rootId, transportAction.remote);
              }
            }}
          >
            {transportAction.label}
          </button>
        ) : null}
      </div>
      {transportAction?.reason ? (
        <p id={reasonId} className="source-root__action-reason">
          {transportAction.reason}
        </p>
      ) : null}
      {showCommitControl ? (
        <RepositoryCommitControl
          repository={repository}
          rootLabel={rootLabel}
          controlsDisabled={controlsDisabled}
        />
      ) : null}
    </>
  );
}
