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
  const partitioned = partitionSourceChanges(changes.slice(0, 300));
  const staged = partitioned.others.filter(
    (change) => change.indexStatus !== "unchanged",
  );
  const unstaged = partitioned.others.filter(
    (change) => change.worktreeStatus !== "unchanged",
  );
  const conflicts = partitioned.conflicts;
  const visibleStaged = staged.slice(0, 300 - conflicts.length);
  const visibleUnstaged = unstaged.slice(
    0,
    300 - conflicts.length - visibleStaged.length,
  );
  const overflow =
    changes.length > 300 ||
    partitioned.conflicts.length + staged.length + unstaged.length > 300;
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
  ) =>
    entries.length > 0 ? (
      <details className="source-change-group" open>
        <summary>
          <ChevronRight size={14} aria-hidden="true" />
          <span>{label}</span>
          {action && onGroupAction ? (
            <button
              type="button"
              className="source-change__action"
              aria-label={
                action === "stage" ? "Stage changes" : "Unstage All Changes"
              }
              title={
                action === "stage" ? "Stage All Changes" : "Unstage All Changes"
              }
              disabled={controlsDisabled}
              onClick={(event) => {
                event.preventDefault();
                onGroupAction(action);
              }}
            >
              {action === "stage" ? (
                <Plus size={14} aria-hidden="true" />
              ) : (
                <Minus size={14} aria-hidden="true" />
              )}
            </button>
          ) : null}
          <span
            className="source-change-group__count"
            aria-label={`${entries.length} ${label.toLowerCase()}`}
          >
            {entries.length}
          </span>
        </summary>
        {entries.map((change) => renderChange(change, action === "unstage"))}
      </details>
    ) : null;
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
      {group("Staged Changes", visibleStaged, "unstage")}
      {group("Changes", visibleUnstaged, "stage")}
      {overflow || truncated ? (
        <p className="tree__limit">Change list capped at 300 files.</p>
      ) : null}
    </>
  );
}
