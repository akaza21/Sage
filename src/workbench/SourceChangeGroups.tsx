import { resourceKey, sameResource } from "../domain/resourceIdentity";
import {
  conflictGroupLabel,
  partitionSourceChanges,
} from "../features/git/conflictPresentation";
import type {
  FileChange,
  GitFileMutationAction,
  RepositoryRef,
  ResourceRef,
} from "../domain/workbench";
import { SourceChangeRow } from "./SourceChangeRow";

export function SourceChangeGroups({
  changes,
  repository,
  rootName,
  rebaseInProgress,
  cherryPickInProgress,
  revertInProgress,
  selected,
  controlsDisabled,
  mutateRepositoryFile,
  openDiff,
}: {
  changes: readonly FileChange[];
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
}) {
  const { conflicts, others } = partitionSourceChanges(changes);
  const staged = others.filter((change) => change.indexStatus !== "unchanged");
  const unstaged = others.filter(
    (change) => change.indexStatus === "unchanged",
  );
  const renderChange = (change: FileChange) => (
    <SourceChangeRow
      key={`${resourceKey(change.resource)}:${change.indexStatus}:${change.worktreeStatus}:${change.originalResource ? resourceKey(change.originalResource) : ""}`}
      change={change}
      repository={repository}
      rootName={rootName}
      isSelected={sameResource(selected, change.resource)}
      controlsDisabled={controlsDisabled}
      mutateRepositoryFile={mutateRepositoryFile}
      openDiff={openDiff}
    />
  );
  const group = (label: string, entries: readonly FileChange[]) =>
    entries.length > 0 ? (
      <details className="source-change-group" open>
        <summary>
          <span>{label}</span>
          <span aria-label={`${entries.length} ${label.toLowerCase()}`}>
            {entries.length}
          </span>
        </summary>
        {entries.map(renderChange)}
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
      {group("Staged Changes", staged)}
      {group("Changes", unstaged)}
    </>
  );
}
