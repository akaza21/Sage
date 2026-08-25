import { useEffect, useId, useRef, useState } from "react";
import { Minus, Plus, RefreshCw, RotateCcw } from "lucide-react";

import { useWorkbenchStore } from "../app/store";
import { isConflictedChange, isSafeDiscardChange } from "../domain/gitMutation";
import { isRootRepository, repositoryKey } from "../domain/repositoryInventory";
import { resourceKey, sameResource } from "../domain/resourceIdentity";
import type {
  FileChange,
  GitFileMutationAction,
  GitFileState,
  RepositoryRef,
  ResourceRef,
} from "../domain/workbench";
import { RepositoryCommitControl } from "./RepositoryCommitControl";
import { SourceBranchList } from "./SourceBranchList";

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

function disambiguatedRootLabel(
  rootName: string,
  ordinal: number,
  rootNameCounts: ReadonlyMap<string, number>,
): string {
  return (rootNameCounts.get(rootName) ?? 0) > 1
    ? `${rootName} (root ${ordinal + 1})`
    : rootName;
}

interface SourceChangeRowProps {
  change: FileChange;
  repository: RepositoryRef | undefined;
  rootName: string;
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
}

function SourceChangeRow({
  change,
  repository,
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
        onClick={() => void openDiff(change.resource, repository)}
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
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const repositories = useWorkbenchStore((state) => state.repositories);
  const repositoryRefs = useWorkbenchStore((state) => state.repositoryRefs);
  const selectedRepository = useWorkbenchStore(
    (state) => state.selectedRepository,
  );
  const selectedRepositoryStatus = useWorkbenchStore(
    (state) => state.selectedRepositoryStatus,
  );
  const inventoryPhase = useWorkbenchStore(
    (state) => state.repositoryInventoryPhase,
  );
  const inventoryError = useWorkbenchStore(
    (state) => state.repositoryInventoryError,
  );
  const statusPhase = useWorkbenchStore((state) => state.repositoryStatusPhase);
  const statusError = useWorkbenchStore((state) => state.repositoryStatusError);
  const selected = useWorkbenchStore((state) => state.selectedResource);
  const refreshing = useWorkbenchStore((state) => state.repositoryRefreshing);
  const mutation = useWorkbenchStore((state) => state.repositoryMutation);
  const commit = useWorkbenchStore((state) => state.repositoryCommit);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const refresh = useWorkbenchStore((state) => state.refreshRepository);
  const loadRepositoryInventory = useWorkbenchStore(
    (state) => state.loadRepositoryInventory,
  );
  const selectRepository = useWorkbenchStore((state) => state.selectRepository);
  const mutateRepositoryFile = useWorkbenchStore(
    (state) => state.mutateRepositoryFile,
  );
  const openDiff = useWorkbenchStore((state) => state.openDiff);
  const controlsDisabled =
    refreshing || transitioning || mutation !== null || commit !== null;
  const selectedRootStatus =
    selectedRepository && isRootRepository(selectedRepository)
      ? (repositories.roots.find(
          (repository) => repository.rootId === selectedRepository.rootId,
        ) ?? null)
      : selectedRepositoryStatus;
  const selectedRepositories =
    selectedRepository === null
      ? repositories.roots
      : selectedRootStatus
        ? [selectedRootStatus]
        : [];

  useEffect(() => {
    void loadRepositoryInventory();
  }, [
    loadRepositoryInventory,
    nativeTransitionEpoch,
    project?.id,
    projectGeneration,
    transitioning,
  ]);
  const projectRoots = new Map(
    project?.roots.map((root) => [root.id, root]) ?? [],
  );
  const rootNameCounts = new Map<string, number>();
  for (const root of project?.roots ?? []) {
    rootNameCounts.set(root.name, (rootNameCounts.get(root.name) ?? 0) + 1);
  }

  return (
    <>
      <div className="sidebar__heading">
        <span>Source Control</span>
        <button
          type="button"
          aria-label="Refresh Source Control"
          disabled={controlsDisabled}
          onClick={() => {
            void refresh();
            void loadRepositoryInventory();
          }}
        >
          <RefreshCw size={14} aria-hidden="true" />
        </button>
      </div>
      <div className="source-control-list">
        {repositoryRefs.length > 0 ? (
          <label className="source-control-selector">
            <span>Repository</span>
            <select
              aria-label="Select repository"
              value={
                selectedRepository ? repositoryKey(selectedRepository) : ""
              }
              disabled={controlsDisabled || inventoryPhase === "loading"}
              onChange={(event) => {
                const next = repositoryRefs.find(
                  (ref) => repositoryKey(ref) === event.target.value,
                );
                if (next) void selectRepository(next);
              }}
            >
              {repositoryRefs.map((ref) => {
                const projectRoot = projectRoots.get(ref.rootId);
                const rootName = projectRoot?.name ?? "Root";
                const rootLabel = disambiguatedRootLabel(
                  rootName,
                  projectRoot?.ordinal ?? 0,
                  rootNameCounts,
                );
                return (
                  <option key={repositoryKey(ref)} value={repositoryKey(ref)}>
                    {`${rootLabel} — ${ref.relativePath || "Repository root"}`}
                  </option>
                );
              })}
            </select>
          </label>
        ) : null}
        {inventoryError ? (
          <p className="sidebar__empty">{inventoryError}</p>
        ) : null}
        {statusError ? <p className="sidebar__empty">{statusError}</p> : null}
        {inventoryPhase === "loading" || statusPhase === "loading" ? (
          <p className="sidebar__empty">Loading repository status…</p>
        ) : null}
        {selectedRepository && !isRootRepository(selectedRepository) ? (
          <p className="sidebar__empty">
            Nested repository status is read-only in this beta.
          </p>
        ) : null}
        {selectedRepositories.map((repository) => {
          const projectRoot = projectRoots.get(repository.rootId);
          const rootName = projectRoot?.name ?? "Root";
          const rootLabel = disambiguatedRootLabel(
            rootName,
            projectRoot?.ordinal ?? 0,
            rootNameCounts,
          );
          return (
            <section className="source-root" key={repository.rootId}>
              <div className="source-control-list__group">
                <span>{rootLabel}</span>
                <span className="muted">
                  {repository.branch ??
                    (repository.state === "notRepository"
                      ? "Not a repository"
                      : "Unavailable")}
                </span>
              </div>
              <SourceBranchList
                rootLabel={rootLabel}
                branch={repository.branch}
                branches={repository.branches}
                truncated={repository.branchesTruncated}
              />
              {repository.message ? (
                <p className="sidebar__empty">{repository.message}</p>
              ) : null}
              {repository.state === "repository" &&
              (!selectedRepository || isRootRepository(selectedRepository)) ? (
                <RepositoryCommitControl
                  repository={repository}
                  rootLabel={rootLabel}
                  controlsDisabled={controlsDisabled}
                />
              ) : null}
              {repository.changes.slice(0, 300).map((change) => {
                return (
                  <SourceChangeRow
                    key={`${resourceKey(change.resource)}:${change.indexStatus}:${change.worktreeStatus}:${change.originalResource ? resourceKey(change.originalResource) : ""}`}
                    change={change}
                    repository={selectedRepository ?? undefined}
                    rootName={rootLabel}
                    selected={selected}
                    controlsDisabled={controlsDisabled}
                    mutateRepositoryFile={mutateRepositoryFile}
                    openDiff={openDiff}
                  />
                );
              })}
              {repository.truncated && !repository.message ? (
                <p className="tree__limit">Change list capped at 300 files.</p>
              ) : null}
            </section>
          );
        })}
      </div>
    </>
  );
}
