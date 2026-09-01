import { useEffect } from "react";
import { RefreshCw } from "lucide-react";

import { useWorkbenchStore } from "../app/store";
import { isRootRepository, repositoryKey } from "../domain/repositoryInventory";
import { resourceKey } from "../domain/resourceIdentity";
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
import { RepositoryCommitControl } from "./RepositoryCommitControl";
import { SourceBranchList } from "./SourceBranchList";
import { SourceChangeRow } from "./SourceChangeRow";
import { SourceCommitList } from "./SourceCommitList";
import { SourceRemoteList } from "./SourceRemoteList";

function SourceChangeGroups({
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
  const renderChange = (change: FileChange) => (
    <SourceChangeRow
      key={`${resourceKey(change.resource)}:${change.indexStatus}:${change.worktreeStatus}:${change.originalResource ? resourceKey(change.originalResource) : ""}`}
      change={change}
      repository={repository}
      rootName={rootName}
      selected={selected}
      controlsDisabled={controlsDisabled}
      mutateRepositoryFile={mutateRepositoryFile}
      openDiff={openDiff}
    />
  );
  return (
    <>
      {conflicts.length > 0 ? (
        <>
          <p className="source-group-label">
            {conflictGroupLabel({
              rebaseInProgress,
              cherryPickInProgress,
              revertInProgress,
            })}
          </p>
          {conflicts.map(renderChange)}
        </>
      ) : rebaseInProgress || cherryPickInProgress || revertInProgress ? (
        <p className="sidebar__empty">Conflicts are resolved.</p>
      ) : null}
      {others.map(renderChange)}
    </>
  );
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
  const checkout = useWorkbenchStore((state) => state.repositoryCheckout);
  const checkoutError = useWorkbenchStore(
    (state) => state.repositoryCheckoutError,
  );
  const remote = useWorkbenchStore((state) => state.repositoryRemote);
  const remoteError = useWorkbenchStore((state) => state.repositoryRemoteError);
  const checkoutLocalBranch = useWorkbenchStore(
    (state) => state.checkoutLocalBranch,
  );
  const createLocalBranch = useWorkbenchStore(
    (state) => state.createLocalBranch,
  );
  const deleteLocalBranch = useWorkbenchStore(
    (state) => state.deleteLocalBranch,
  );
  const renameLocalBranch = useWorkbenchStore(
    (state) => state.renameLocalBranch,
  );
  const fetchRemote = useWorkbenchStore((state) => state.fetchRemote);
  const pushRemote = useWorkbenchStore((state) => state.pushRemote);
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
    refreshing ||
    transitioning ||
    mutation !== null ||
    commit !== null ||
    checkout !== null ||
    remote !== null;
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
            <span className="sr-only">Repository</span>
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
          <p className="sidebar__error" role="alert">
            {inventoryError}
          </p>
        ) : null}
        {statusError ? (
          <p className="sidebar__error" role="alert">
            {statusError}
          </p>
        ) : null}
        {inventoryPhase === "loading" || statusPhase === "loading" ? (
          <p className="sidebar__loading" role="status">
            Loading repository status…
          </p>
        ) : null}
        {selectedRepository && !isRootRepository(selectedRepository) ? (
          <p className="sidebar__notice">
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
              {repository.state === "repository" &&
              (!selectedRepository || isRootRepository(selectedRepository)) ? (
                <RepositoryCommitControl
                  repository={repository}
                  rootLabel={rootLabel}
                  controlsDisabled={controlsDisabled}
                />
              ) : null}
              <SourceBranchList
                rootLabel={rootLabel}
                branch={repository.branch}
                branches={repository.branches}
                truncated={repository.branchesTruncated}
                checkoutDisabled={controlsDisabled}
                createDisabled={controlsDisabled}
                deleteDisabled={controlsDisabled}
                renameDisabled={controlsDisabled}
                onCheckout={
                  repository.state === "repository" &&
                  (!selectedRepository || isRootRepository(selectedRepository))
                    ? (name) =>
                        void checkoutLocalBranch(repository.rootId, name)
                    : undefined
                }
                onCreate={
                  repository.state === "repository" &&
                  (!selectedRepository || isRootRepository(selectedRepository))
                    ? (name) => void createLocalBranch(repository.rootId, name)
                    : undefined
                }
                onDelete={
                  repository.state === "repository" &&
                  (!selectedRepository || isRootRepository(selectedRepository))
                    ? (name) => void deleteLocalBranch(repository.rootId, name)
                    : undefined
                }
                onRename={
                  repository.state === "repository" &&
                  (!selectedRepository || isRootRepository(selectedRepository))
                    ? (from, to) =>
                        void renameLocalBranch(repository.rootId, from, to)
                    : undefined
                }
              />
              <SourceCommitList
                rootLabel={rootLabel}
                commits={repository.recentCommits}
                truncated={repository.recentCommitsTruncated}
              />
              <SourceRemoteList
                rootLabel={rootLabel}
                remotes={repository.remotes}
                truncated={repository.remotesTruncated}
                repository={
                  repository.state === "repository" &&
                  (!selectedRepository || isRootRepository(selectedRepository))
                    ? repository
                    : undefined
                }
                transportDisabled={controlsDisabled}
                onFetch={
                  repository.state === "repository" &&
                  (!selectedRepository || isRootRepository(selectedRepository))
                    ? (name) => void fetchRemote(repository.rootId, name)
                    : undefined
                }
                onPush={
                  repository.state === "repository" &&
                  (!selectedRepository || isRootRepository(selectedRepository))
                    ? (name) => void pushRemote(repository.rootId, name)
                    : undefined
                }
              />
              {checkoutError?.rootId === repository.rootId ? (
                <p className="sidebar__error" role="alert">
                  {checkoutError.message}
                </p>
              ) : null}
              {remoteError?.rootId === repository.rootId ? (
                <p className="sidebar__error" role="alert">
                  {remoteError.message}
                </p>
              ) : null}
              {repository.message ? (
                <p
                  className={
                    repository.state === "unavailable"
                      ? "sidebar__error"
                      : "sidebar__notice"
                  }
                  role={
                    repository.state === "unavailable" ? "alert" : undefined
                  }
                >
                  {repository.message}
                </p>
              ) : null}
              <SourceChangeGroups
                changes={repository.changes.slice(0, 300)}
                repository={selectedRepository ?? undefined}
                rootName={rootLabel}
                rebaseInProgress={repository.rebaseInProgress === true}
                cherryPickInProgress={repository.cherryPickInProgress === true}
                revertInProgress={repository.revertInProgress === true}
                selected={selected}
                controlsDisabled={controlsDisabled}
                mutateRepositoryFile={mutateRepositoryFile}
                openDiff={openDiff}
              />
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
