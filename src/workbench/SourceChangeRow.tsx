import { memo, useId, useRef, useState } from "react";
import { Minus, Plus, RotateCcw } from "../ui/leanIcons";

import { isConflictedChange, isSafeDiscardChange } from "../domain/gitMutation";
import { isRootRepository } from "../domain/repositoryInventory";
import { sourceChangeStatusChip } from "../features/git/conflictPresentation";
import type {
  FileChange,
  GitFileMutationAction,
  RepositoryRef,
  ResourceRef,
} from "../domain/workbench";

interface SourceChangeRowProps {
  change: FileChange;
  repository: RepositoryRef | undefined;
  rootName: string;
  isSelected: boolean;
  controlsDisabled: boolean;
  mutateRepositoryFile: (
    resource: ResourceRef,
    action: GitFileMutationAction,
    discardConfirmed?: boolean,
  ) => Promise<boolean>;
  openDiff: (
    resource: ResourceRef,
    repository?: RepositoryRef,
  ) => Promise<void>;
}

export const SourceChangeRow = memo(function SourceChangeRow({
  change,
  repository,
  rootName,
  isSelected,
  controlsDisabled,
  mutateRepositoryFile,
  openDiff,
}: SourceChangeRowProps) {
  const [confirmingDiscard, setConfirmingDiscard] = useState(false);
  const discardTriggerRef = useRef<HTMLButtonElement>(null);
  const discardWarningId = useId();
  const conflicted = isConflictedChange(change);
  const readOnly = repository !== undefined && !isRootRepository(repository);
  const canStage =
    !readOnly &&
    !conflicted &&
    change.worktreeStatus !== "unchanged" &&
    change.worktreeStatus !== "unknown";
  const canUnstage =
    !readOnly &&
    !conflicted &&
    change.indexStatus !== "unchanged" &&
    change.indexStatus !== "unknown";
  const canDiscard = !readOnly && isSafeDiscardChange(change);
  const canResolve = !readOnly && conflicted;
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
          isSelected ? "source-change--selected" : ""
        }${conflicted ? " source-change--conflicted" : ""}`}
        type="button"
        aria-label={`Open diff for ${path}`}
        aria-current={isSelected ? "true" : undefined}
        onClick={() => void openDiff(change.resource, repository)}
      >
        <span
          className={`source-change__status${
            conflicted ? " source-change__status--conflict" : ""
          }`}
        >
          {sourceChangeStatusChip(change)}
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
          {canResolve ? (
            <>
              <button
                className="source-change__resolve"
                type="button"
                aria-label={`Keep ours for ${path} in ${rootName}`}
                title="Keep ours"
                disabled={controlsDisabled}
                onClick={() =>
                  void mutateRepositoryFile(change.resource, "keepOurs")
                }
              >
                Ours
              </button>
              <button
                className="source-change__resolve"
                type="button"
                aria-label={`Keep theirs for ${path} in ${rootName}`}
                title="Keep theirs"
                disabled={controlsDisabled}
                onClick={() =>
                  void mutateRepositoryFile(change.resource, "keepTheirs")
                }
              >
                Theirs
              </button>
            </>
          ) : null}
          <span className="source-change__row-actions">
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
                <RotateCcw size={14} aria-hidden="true" />
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
                <Plus size={14} aria-hidden="true" />
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
                <Minus size={14} aria-hidden="true" />
              </button>
            ) : null}
          </span>
        </span>
      )}
    </div>
  );
});
