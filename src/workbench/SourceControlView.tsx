import { useCallback, useEffect, useMemo } from "react";
import { RefreshCw } from "lucide-react";

import { useWorkbenchStore } from "../app/store";
import { isRootRepository, repositoryKey } from "../domain/repositoryInventory";
import { SourceBranchList } from "./SourceBranchList";
import { SourceChangeGroups } from "./SourceChangeGroups";
import { SourceCommitList } from "./SourceCommitList";
import { SourceRemoteList } from "./SourceRemoteList";
import { SourceRepositoryControl } from "./SourceRepositoryControl";
import { SearchablePicker } from "../ui/SearchablePicker";
import { ResourceStateNotice } from "../ui/ResourceStateNotice";

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
  const projectRoots = useMemo(
    () => new Map(project?.roots.map((root) => [root.id, root]) ?? []),
    [project?.roots],
  );
  const rootNameCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const root of project?.roots ?? []) {
      counts.set(root.name, (counts.get(root.name) ?? 0) + 1);
    }
    return counts;
  }, [project?.roots]);
  const repositoryOptions = useMemo(
    () =>
      repositoryRefs.map((ref) => {
        const projectRoot = projectRoots.get(ref.rootId);
        const rootName = projectRoot?.name ?? "Root";
        const rootLabel = disambiguatedRootLabel(
          rootName,
          projectRoot?.ordinal ?? 0,
          rootNameCounts,
        );
        return {
          disabledReason:
            projectRoot?.availability === "unavailable"
              ? (projectRoot.message ?? "Repository root is unavailable.")
              : undefined,
          id: repositoryKey(ref),
          label: `${rootLabel} — ${ref.relativePath || "Repository root"}`,
        };
      }),
    [projectRoots, repositoryRefs, rootNameCounts],
  );
  const chooseRepository = useCallback(
    (key: string) => {
      if (controlsDisabled || inventoryPhase === "loading") return;
      const next = repositoryRefs.find((ref) => repositoryKey(ref) === key);
      if (next) void selectRepository(next);
    },
    [controlsDisabled, inventoryPhase, repositoryRefs, selectRepository],
  );

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
          <div className="source-control-selector">
            <SearchablePicker
              ariaLabel="Select repository"
              candidates={repositoryOptions}
              disabled={controlsDisabled || inventoryPhase === "loading"}
              onSelect={chooseRepository}
              selectedId={
                selectedRepository ? repositoryKey(selectedRepository) : null
              }
            />
          </div>
        ) : null}
        {inventoryError || statusError ? (
          <ResourceStateNotice
            detail={inventoryError ?? statusError ?? undefined}
            state="error"
            title="Repository status is unavailable"
          />
        ) : inventoryPhase === "loading" || statusPhase === "loading" ? (
          <ResourceStateNotice
            state="loading"
            title="Loading repository status"
          />
        ) : inventoryPhase === "ready" && repositoryRefs.length === 0 ? (
          <ResourceStateNotice state="idle" title="No repositories found" />
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
          const writable =
            repository.state === "repository" &&
            (!selectedRepository || isRootRepository(selectedRepository));
          return (
            <section className="source-root" key={repository.rootId}>
              <SourceRepositoryControl
                repository={repository}
                rootLabel={rootLabel}
                controlsDisabled={controlsDisabled}
                writable={writable}
              />
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
                  writable
                    ? (name) =>
                        void checkoutLocalBranch(repository.rootId, name)
                    : undefined
                }
                onCreate={
                  writable
                    ? (name) => void createLocalBranch(repository.rootId, name)
                    : undefined
                }
                onDelete={
                  writable
                    ? (name) => void deleteLocalBranch(repository.rootId, name)
                    : undefined
                }
                onRename={
                  writable
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
                repository={writable ? repository : undefined}
                transportDisabled={controlsDisabled}
                onFetch={
                  writable
                    ? (name) => void fetchRemote(repository.rootId, name)
                    : undefined
                }
                onPush={
                  writable
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
