import { useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { GitBranch } from "../../ui/leanIcons";
import type * as BranchChunk from "../../workbench/StatusBarDetails";
import { lazyParts } from "./lazyParts";

const BranchPart = lazyParts<typeof BranchChunk>(
  () => import("../../workbench/StatusBarDetails"),
);

export function ComposerBranchChip({
  disabled: submissionBusy,
  activeTurn = false,
}: {
  disabled: boolean;
  activeTurn?: boolean;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const repositories = useWorkbenchStore((state) => state.repositories);
  const targetId = useWorkbenchStore((state) => {
    const draft = state.agentCenter === "draft" ? state.newChatDraft : null;
    return draft
      ? draft.targetProjectId === undefined
        ? draft.projectId
        : draft.targetProjectId
      : state.project?.isInternal
        ? null
        : state.project?.id;
  });
  const busy = useWorkbenchStore(
    (state) =>
      state.projectTransitioning ||
      state.repositoryRefreshing ||
      !!state.repositoryCheckout ||
      !!state.repositoryMutation ||
      !!state.repositoryCommit ||
      !!state.repositoryRemote,
  );
  // Git commands are scoped to the loaded project. Never show that project's
  // branch as if it belonged to a draft targeting another project.
  const root = project && project.id === targetId ? project.roots[0] : null;
  const repository = repositories.roots.find(
    (entry) => entry.rootId === root?.id,
  );
  const [loadState, setLoadState] = useState<"idle" | "requested" | "failed">(
    "idle",
  );
  const [open, setOpen] = useState(false);
  if (repository?.state !== "repository" || !root) return null;
  const disabled = submissionBusy || activeTurn || busy;
  const title = activeTurn
    ? "Can't switch branches while the agent is working"
    : loadState === "failed"
      ? "Couldn't load branches. Click to retry."
      : undefined;
  const label = (
    <>
      <GitBranch size={14} aria-hidden="true" />
      <span className="searchable-picker__trigger-label">
        {repository.branch ?? "Repository"}
      </span>
    </>
  );
  const fallback = (
    <button
      type="button"
      className="searchable-picker__trigger searchable-picker__trigger--plain"
      aria-label={`${loadState === "failed" ? "Retry loading branches" : "Switch branch"} in ${root.name}`}
      aria-haspopup="listbox"
      aria-expanded={open && !disabled}
      title={title}
      disabled={disabled}
      onClick={() => {
        setLoadState("requested");
        setOpen(true);
      }}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          setLoadState("requested");
          setOpen(true);
        }
      }}
    >
      {label}
    </button>
  );
  return (
    <div className="composer-branch" title={title}>
      {loadState === "requested" ? (
        <BranchPart
          part="SourceBranchList"
          key={`${project?.id}:${root.id}`}
          repository={repository}
          rootLabel={root.name}
          disabled={disabled}
          stateTitle={title}
          triggerClassName="composer-branch__trigger"
          triggerLabel={label}
          manageBranches={false}
          open={open}
          onOpenChange={setOpen}
          fallback={fallback}
          onLoadFailed={() => {
            setOpen(false);
            setLoadState("failed");
          }}
        />
      ) : (
        fallback
      )}
    </div>
  );
}
