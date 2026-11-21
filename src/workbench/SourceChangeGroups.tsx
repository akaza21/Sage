import { useState, type ReactNode } from "react";
import { resourceKey, sameResource } from "../domain/resourceIdentity";
import {
  type FileChange,
  type GitFileMutationAction,
  type RepositoryRef,
  type ResourceRef,
} from "../domain/workbench";
import {
  conflictGroupLabel,
  partitionSourceChanges,
} from "../features/git/conflictPresentation";
import { SourceChangeRow } from "./SourceChangeRow";
import { ChevronRight, Minus, Plus } from "../ui/leanIcons";

export function SourceChangeGroups({
  changes,
  truncated = false,
  repository,
  rootName,
  rebaseInProgress,
  cherryPickInProgress,
  revertInProgress,
  selected,
  controlsDisabled,
  mutateRepositoryFile,
  openDiff,
  onGroupAction,
}: {
  changes: readonly FileChange[];
  truncated?: boolean;
  repository: RepositoryRef | undefined;
  rootName: string;
  rebaseInProgress: boolean;
  cherryPickInProgress: boolean;
  revertInProgress: boolean;
  selected: ResourceRef | null;
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
  onGroupAction?: (action: "stage" | "unstage") => void;
}) {
  const { conflicts, others } = partitionSourceChanges(changes.slice(0, 300));
  const staged = others.filter((change) => change.indexStatus !== "unchanged");
  const unstaged = others.filter(
    (change) => change.worktreeStatus !== "unchanged",
  );
  const pageSize = Math.floor(
    300 /
      ([conflicts, staged, unstaged].filter((entries) => entries.length)
        .length || 1),
  );
  const renderChange = (change: FileChange, stagedGroup: boolean) => (
    <SourceChangeRow
      key={`${resourceKey(change.resource)}:${change.indexStatus}:${change.worktreeStatus}:${change.originalResource ? resourceKey(change.originalResource) : ""}`}
      change={change}
      stagedGroup={stagedGroup}
      repository={repository}
      rootName={rootName}
      isSelected={sameResource(selected, change.resource)}
      controlsDisabled={controlsDisabled}
      mutateRepositoryFile={mutateRepositoryFile}
      openDiff={openDiff}
    />
  );
  const group = (
    label: string,
    entries: readonly FileChange[],
    action?: "stage" | "unstage",
  ) => {
    const staging = action === "stage";
    return entries.length ? (
      <SourceChangeGroup
        label={label}
        entries={entries}
        pageSize={pageSize}
        action={
          action && onGroupAction ? (
            <button
              type="button"
              className="source-change__action"
              aria-label={staging ? "Stage changes" : "Unstage All Changes"}
              title={staging ? "Stage All Changes" : "Unstage All Changes"}
              disabled={controlsDisabled}
              onClick={() => onGroupAction(action)}
            >
              {staging ? (
                <Plus size={14} aria-hidden="true" />
              ) : (
                <Minus size={14} aria-hidden="true" />
              )}
            </button>
          ) : null
        }
      >
        {(change) => renderChange(change, action === "unstage")}
      </SourceChangeGroup>
    ) : null;
  };
  return (
    <>
      {conflicts.length > 0 ? (
        group(
          conflictGroupLabel({
            rebaseInProgress,
            cherryPickInProgress,
            revertInProgress,
          }),
          conflicts,
        )
      ) : rebaseInProgress || cherryPickInProgress || revertInProgress ? (
        <p className="sidebar__empty">Conflicts are resolved.</p>
      ) : null}
      {group("Staged Changes", staged, "unstage")}
      {group("Changes", unstaged, "stage")}
      {changes.length > 300 || truncated ? (
        <p className="tree__limit">Change list capped at 300 files.</p>
      ) : null}
    </>
  );
}

function SourceChangeGroup({
  label,
  entries,
  pageSize,
  action,
  children,
}: {
  label: string;
  entries: readonly FileChange[];
  pageSize: number;
  action: ReactNode;
  children: (change: FileChange) => ReactNode;
}) {
  const [expanded, setExpanded] = useState(true);
  const [page, setPage] = useState(0);
  const lastPage = Math.floor((entries.length - 1) / pageSize);
  const currentPage = Math.min(page, lastPage);
  const offset = currentPage * pageSize;
  return (
    <details
      className="source-change-group"
      open={expanded}
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      <summary>
        <ChevronRight size={14} aria-hidden="true" />
        <span>{label}</span>
        {action}
        <span className="source-change-group__count">{entries.length}</span>
      </summary>
      {expanded && entries.slice(offset, offset + pageSize).map(children)}
      {expanded && lastPage > 0 && (
        <div className="source-change__confirmation-actions">
          <button
            type="button"
            className="source-change__confirmation-button"
            disabled={currentPage === 0}
            onClick={() => setPage(currentPage - 1)}
          >
            Previous
          </button>
          <button
            type="button"
            className="source-change__confirmation-button"
            disabled={currentPage === lastPage}
            onClick={() => setPage(currentPage + 1)}
          >
            Show more
          </button>
        </div>
      )}
    </details>
  );
}
