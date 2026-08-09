import { Minus, Plus, RefreshCw } from "lucide-react";

import { useWorkbenchStore } from "../app/store";
import { resourceKey, sameResource } from "../domain/resourceIdentity";
import type { FileChange, GitFileState } from "../domain/workbench";

const statusLabels: Record<GitFileState, string> = {
  unchanged: "",
  modified: "M",
  typeChanged: "T",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  conflicted: "!",
  untracked: "U",
  unknown: "?",
};

function visibleStatus(change: FileChange): GitFileState {
  return change.worktreeStatus !== "unchanged"
    ? change.worktreeStatus
    : change.indexStatus;
}

export function SourceControlView() {
  const project = useWorkbenchStore((state) => state.project);
  const repositories = useWorkbenchStore((state) => state.repositories);
  const selected = useWorkbenchStore((state) => state.selectedResource);
  const refreshing = useWorkbenchStore((state) => state.repositoryRefreshing);
  const mutation = useWorkbenchStore((state) => state.repositoryMutation);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const refresh = useWorkbenchStore((state) => state.refreshRepository);
  const mutateRepositoryFile = useWorkbenchStore(
    (state) => state.mutateRepositoryFile,
  );
  const openDiff = useWorkbenchStore((state) => state.openDiff);
  const controlsDisabled = refreshing || transitioning || mutation !== null;
  const rootNames = new Map(
    project?.roots.map((root) => [root.id, root.name]) ?? [],
  );

  return (
    <>
      <div className="sidebar__heading">
        <span>Source Control</span>
        <button
          type="button"
          aria-label="Refresh Source Control"
          disabled={controlsDisabled}
          onClick={() => void refresh()}
        >
          <RefreshCw size={14} aria-hidden="true" />
        </button>
      </div>
      <div className="source-control-list">
        {repositories.roots.map((repository) => {
          const rootName = rootNames.get(repository.rootId) ?? "Root";
          return (
            <section className="source-root" key={repository.rootId}>
              <div className="source-control-list__group">
                <span>{rootName}</span>
                <span className="muted">
                  {repository.branch ??
                    (repository.state === "notRepository"
                      ? "Not a repository"
                      : "Unavailable")}
                </span>
              </div>
              {repository.message ? (
                <p className="sidebar__empty">{repository.message}</p>
              ) : null}
              {repository.changes.slice(0, 300).map((change) => {
                const conflicted =
                  change.indexStatus === "conflicted" ||
                  change.worktreeStatus === "conflicted";
                const canStage =
                  !conflicted &&
                  change.worktreeStatus !== "unchanged" &&
                  change.worktreeStatus !== "unknown";
                const canUnstage =
                  !conflicted &&
                  change.indexStatus !== "unchanged" &&
                  change.indexStatus !== "unknown";
                return (
                  <div
                    className="source-change-row"
                    key={resourceKey(change.resource)}
                  >
                    <button
                      className={`source-change ${
                        sameResource(selected, change.resource)
                          ? "source-change--selected"
                          : ""
                      }`}
                      type="button"
                      aria-label={`Open diff for ${change.resource.relativePath}`}
                      onClick={() => void openDiff(change.resource)}
                    >
                      <span className="source-change__status">
                        {statusLabels[visibleStatus(change)]}
                      </span>
                      <span className="source-change__path">
                        {change.resource.relativePath}
                      </span>
                      <span className="source-change__counts">
                        {change.additions === null
                          ? ""
                          : `+${change.additions}`}
                        {change.deletions === null
                          ? ""
                          : ` -${change.deletions}`}
                      </span>
                    </button>
                    <span className="source-change__actions">
                      {canStage ? (
                        <button
                          className="source-change__action"
                          type="button"
                          aria-label={`Stage ${change.resource.relativePath} in ${rootName}`}
                          title="Stage file"
                          disabled={controlsDisabled}
                          onClick={() =>
                            void mutateRepositoryFile(change.resource, "stage")
                          }
                        >
                          <Plus size={13} aria-hidden="true" />
                        </button>
                      ) : null}
                      {canUnstage ? (
                        <button
                          className="source-change__action"
                          type="button"
                          aria-label={`Unstage ${change.resource.relativePath} in ${rootName}`}
                          title="Unstage file"
                          disabled={controlsDisabled}
                          onClick={() =>
                            void mutateRepositoryFile(
                              change.resource,
                              "unstage",
                            )
                          }
                        >
                          <Minus size={13} aria-hidden="true" />
                        </button>
                      ) : null}
                    </span>
                  </div>
                );
              })}
              {repository.truncated ? (
                <p className="tree__limit">Change list capped at 300 files.</p>
              ) : null}
            </section>
          );
        })}
      </div>
    </>
  );
}
