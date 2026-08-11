import { useId, useRef, useState } from "react";
import { Minus, Plus, RefreshCw, RotateCcw } from "lucide-react";

import { useWorkbenchStore } from "../app/store";
import { isConflictedChange, isSafeDiscardChange } from "../domain/gitMutation";
import { resourceKey, sameResource } from "../domain/resourceIdentity";
import type {
  FileChange,
  GitFileMutationAction,
  GitFileState,
  ResourceRef,
} from "../domain/workbench";
import { RepositoryCommitControl } from "./RepositoryCommitControl";

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

interface SourceChangeRowProps {
  change: FileChange;
  rootName: string;
  selected: ResourceRef | null;
  controlsDisabled: boolean;
  mutateRepositoryFile: (
    resource: ResourceRef,
    action: GitFileMutationAction,
    discardConfirmed?: boolean,
  ) => Promise<boolean>;
  openDiff: (resource: ResourceRef) => Promise<void>;
}

function SourceChangeRow({
  change,
  rootName,
  selected,
  controlsDisabled,
  mutateRepositoryFile,
  openDiff,
}: SourceChangeRowProps) {
  const [confirmingDiscard, setConfirmingDiscard] = useState(false);
  const discardTriggerRef = useRef<HTMLButtonElement>(null);
  const discardWarningId = useId();
  const conflicted = isConflictedChange(change);
  const canStage =
    !conflicted &&
    change.worktreeStatus !== "unchanged" &&
    change.worktreeStatus !== "unknown";
  const canUnstage =
    !conflicted &&
    change.indexStatus !== "unchanged" &&
    change.indexStatus !== "unknown";
  const canDiscard = isSafeDiscardChange(change);
  const path = change.resource.relativePath;
  const cancelDiscard = () => {
    setConfirmingDiscard(false);
    queueMicrotask(() => discardTriggerRef.current?.focus());
  };

  return (
    <div
      className={`source-change-row ${
        confirmingDiscard ? "source-change-row--confirming" : ""
      }`}
    >
      <button
        className={`source-change ${
          sameResource(selected, change.resource)
            ? "source-change--selected"
            : ""
        }`}
        type="button"
        aria-label={`Open diff for ${path}`}
        onClick={() => void openDiff(change.resource)}
      >
        <span className="source-change__status">
          {statusLabels[visibleStatus(change)]}
        </span>
        <span className="source-change__path">{path}</span>
        <span className="source-change__counts">
          {change.additions === null ? "" : `+${change.additions}`}
          {change.deletions === null ? "" : ` -${change.deletions}`}
        </span>
      </button>
      {confirmingDiscard && canDiscard ? (
        <div
          className="source-change__discard-confirmation"
          role="group"
          aria-label={`Confirm discard of ${path} in ${rootName}`}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              cancelDiscard();
            }
          }}
        >
          <span id={discardWarningId}>
            Discard unstaged changes to {path} in {rootName}? This cannot be
            undone.
          </span>
          <span className="source-change__confirmation-actions">
            <button
              className="source-change__confirmation-button source-change__confirmation-button--danger"
              type="button"
              aria-label={`Confirm discard of ${path} in ${rootName}`}
              aria-describedby={discardWarningId}
              disabled={controlsDisabled}
              onClick={() => {
                setConfirmingDiscard(false);
                void mutateRepositoryFile(change.resource, "discard", true);
              }}
            >
              Discard
            </button>
            <button
              className="source-change__confirmation-button"
              type="button"
              aria-label={`Cancel discard of ${path} in ${rootName}`}
              aria-describedby={discardWarningId}
              autoFocus
              onClick={cancelDiscard}
            >
              Cancel
            </button>
          </span>
        </div>
      ) : (
        <span className="source-change__actions">
          {canDiscard ? (
            <button
              className="source-change__action"
              type="button"
              aria-label={`Discard unstaged changes to ${path} in ${rootName}`}
              title="Discard unstaged changes"
              ref={discardTriggerRef}
              disabled={controlsDisabled}
              onClick={() => setConfirmingDiscard(true)}
            >
              <RotateCcw size={13} aria-hidden="true" />
            </button>
          ) : null}
          {canStage ? (
            <button
              className="source-change__action"
              type="button"
              aria-label={`Stage ${path} in ${rootName}`}
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
              aria-label={`Unstage ${path} in ${rootName}`}
              title="Unstage file"
              disabled={controlsDisabled}
              onClick={() =>
                void mutateRepositoryFile(change.resource, "unstage")
              }
            >
              <Minus size={13} aria-hidden="true" />
            </button>
          ) : null}
        </span>
      )}
    </div>
  );
}

export function SourceControlView() {
  const project = useWorkbenchStore((state) => state.project);
  const repositories = useWorkbenchStore((state) => state.repositories);
  const selected = useWorkbenchStore((state) => state.selectedResource);
  const refreshing = useWorkbenchStore((state) => state.repositoryRefreshing);
  const mutation = useWorkbenchStore((state) => state.repositoryMutation);
  const commit = useWorkbenchStore((state) => state.repositoryCommit);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const refresh = useWorkbenchStore((state) => state.refreshRepository);
  const mutateRepositoryFile = useWorkbenchStore(
    (state) => state.mutateRepositoryFile,
  );
  const openDiff = useWorkbenchStore((state) => state.openDiff);
  const controlsDisabled =
    refreshing || transitioning || mutation !== null || commit !== null;
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
              {repository.state === "repository" ? (
                <RepositoryCommitControl
                  repository={repository}
                  rootName={rootName}
                  controlsDisabled={controlsDisabled}
                  projectStatusTruncated={repositories.truncated}
                />
              ) : null}
              {repository.changes.slice(0, 300).map((change) => {
                return (
                  <SourceChangeRow
                    key={`${resourceKey(change.resource)}:${change.indexStatus}:${change.worktreeStatus}:${change.originalResource ? resourceKey(change.originalResource) : ""}`}
                    change={change}
                    rootName={rootName}
                    selected={selected}
                    controlsDisabled={controlsDisabled}
                    mutateRepositoryFile={mutateRepositoryFile}
                    openDiff={openDiff}
                  />
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
