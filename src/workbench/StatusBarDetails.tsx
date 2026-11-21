import { useState } from "react";
import { useWorkbenchStore } from "../app/store";
import { GitBranch } from "../ui/leanIcons";
import type { ProjectRepositoryStatus } from "../domain/workbench";
import { SourceBranchList } from "./SourceBranchList";
export { SourceBranchList } from "./SourceBranchList";

function repositoryStatusLabel(repositories: ProjectRepositoryStatus) {
  const roots = repositories.roots;
  if (roots.length === 0) return "No project";
  if (roots.length > 1) {
    const count = roots.filter((root) => root.state === "repository").length;
    return `${count} ${count === 1 ? "repository" : "repositories"} across ${roots.length} roots`;
  }
  const [repository] = roots;
  if (repository.state === "repository")
    return repository.branch ?? "Repository";
  return repository.state === "notRepository"
    ? "No repository"
    : "Repository unavailable";
}

export default function SourceBranchStatus() {
  const repositories = useWorkbenchStore((state) => state.repositories);
  const selectedRepository = useWorkbenchStore(
    (state) => state.selectedRepository,
  );
  const selectedStatus = useWorkbenchStore(
    (state) => state.selectedRepositoryStatus,
  );
  const selectedResource = useWorkbenchStore((state) => state.selectedResource);
  const project = useWorkbenchStore((state) => state.project);
  const busy = useWorkbenchStore(
    (state) =>
      state.projectTransitioning ||
      state.repositoryRefreshing ||
      !!state.repositoryCheckout ||
      !!state.repositoryMutation ||
      !!state.repositoryCommit ||
      !!state.repositoryRemote,
  );
  const [branchesOpen, setBranchesOpen] = useState(false);
  const repository = selectedRepository?.relativePath
    ? selectedStatus
    : (repositories.roots.find(
        (root) =>
          root.rootId ===
          (selectedRepository?.rootId ?? selectedResource?.rootId),
      ) ?? (repositories.roots.length === 1 ? repositories.roots[0] : null));
  const rootLabel =
    project?.roots.find((root) => root.id === repository?.rootId)?.name ??
    "Repository";
  const branchLabel = (
    <>
      <GitBranch size={14} aria-hidden="true" />
      <span>{repository?.branch ?? repositoryStatusLabel(repositories)}</span>
    </>
  );
  return (
    <>
      {repository?.state === "repository" &&
      !selectedRepository?.relativePath ? (
        <div className="status-bar__branch">
          <SourceBranchList
            key={`${project?.id}:${repository.rootId}`}
            repository={repository}
            rootLabel={rootLabel}
            disabled={busy}
            triggerLabel={branchLabel}
            open={branchesOpen}
            onOpenChange={setBranchesOpen}
          />
          {repository.upstream ? (
            <span
              title={`${repository.upstream}: ${repository.ahead ?? 0} ahead, ${repository.behind ?? 0} behind`}
            >
              ↑{repository.ahead ?? 0} ↓{repository.behind ?? 0}
            </span>
          ) : null}
        </div>
      ) : (
        <span>{branchLabel}</span>
      )}
    </>
  );
}
