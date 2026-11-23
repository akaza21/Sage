import { useId, useMemo, useRef, useState, type ReactNode } from "react";
import { useWorkbenchStore } from "../app/store";
import type { RootRepositoryStatus } from "../domain/workbench";
import {
  canCheckoutLocalBranch,
  canCreateLocalBranch,
  canDeleteLocalBranch,
  canRenameLocalBranch,
  isCurrentLocalBranch,
  MAX_LOCAL_BRANCHES,
} from "../features/git/localBranches";
import { MenuPopover } from "../ui/MenuPopover";
import { SearchablePicker } from "../ui/SearchablePicker";
import { ConfirmDialog } from "../ui/ConfirmDialog";

export function SourceBranchList({
  repository,
  rootLabel,
  disabled,
  triggerLabel,
  triggerRole,
  open,
  onOpenChange,
  manageBranches = true,
  triggerClassName,
  stateTitle,
}: {
  repository: RootRepositoryStatus;
  rootLabel: string;
  disabled: boolean;
  triggerLabel: ReactNode;
  triggerRole?: "menuitem";
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  manageBranches?: boolean;
  triggerClassName?: string;
  stateTitle?: string;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const [mode, setMode] = useState<"create" | "rename" | "delete" | null>(null);
  const [draft, setDraft] = useState("");
  const [from, setFrom] = useState("");
  const [checkoutFailed, setCheckoutFailed] = useState(false);
  const [pendingBranch, setPendingBranch] = useState<string | null>(null);
  if (disabled && pendingBranch) setPendingBranch(null);
  const checkout = useWorkbenchStore((state) => state.checkoutLocalBranch);
  const create = useWorkbenchStore((state) => state.createLocalBranch);
  const rename = useWorkbenchStore((state) => state.renameLocalBranch);
  const remove = useWorkbenchStore((state) => state.deleteLocalBranch);
  const error = useWorkbenchStore((state) => state.repositoryCheckoutError);
  const names = useMemo(
    () => (repository.branches ?? []).slice(0, MAX_LOCAL_BRANCHES),
    [repository.branches],
  );
  const branchCandidates = useMemo(
    () => names.map((name) => ({ id: name, label: name })),
    [names],
  );
  const candidates = useMemo(
    () => [
      {
        id: "create",
        label: "Create new branch…",
        disabledReason:
          repository.branchesTruncated || names.length >= MAX_LOCAL_BRANCHES
            ? "Branch list must be complete and below 64 names."
            : undefined,
      },
      ...names.map((name) => ({
        id: `branch:${name}`,
        label: name,
        accessibleLabel: isCurrentLocalBranch(name, repository.branch)
          ? `${name} (current)`
          : `Switch to ${name}`,
        subtitle: isCurrentLocalBranch(name, repository.branch)
          ? "current"
          : undefined,
      })),
      ...(manageBranches
        ? [
            { id: "rename", label: "Rename branch…" },
            { id: "delete", label: "Delete branch…" },
          ]
        : []),
    ],
    [names, repository.branch, repository.branchesTruncated, manageBranches],
  );
  const switchBranch = async (name: string) => {
    if (disabled || !canCheckoutLocalBranch(name, repository.branch, names))
      return;
    setCheckoutFailed(!(await checkout(repository.rootId, name)));
  };
  const valid =
    mode === "create"
      ? canCreateLocalBranch(draft, names, repository.branchesTruncated)
      : mode === "rename"
        ? canRenameLocalBranch(from, draft, names, repository.branchesTruncated)
        : canDeleteLocalBranch(
            from,
            repository.branch,
            names,
            repository.branchesTruncated,
          );
  return (
    <div className="source-branch-picker">
      <SearchablePicker
        ariaLabel={`Switch branch in ${rootLabel}`}
        triggerRef={trigger}
        appearance="plain"
        chevron={false}
        triggerRole={triggerRole}
        triggerClassName={
          triggerClassName ?? (triggerRole ? "menu-item" : "status-bar__button")
        }
        stateTitle={stateTitle}
        triggerLabel={triggerLabel}
        selectedId={`branch:${repository.branch}`}
        candidates={candidates}
        disabled={disabled}
        open={open}
        onOpenChange={onOpenChange}
        popoverPlacement="above-start"
        menuAppearance
        searchPlaceholder="Select a branch"
        footer={
          repository.branchesTruncated
            ? "Branch list capped at 64 names."
            : undefined
        }
        onSelect={async (choice) => {
          if (disabled) return;
          if (
            choice === "create" ||
            choice === "rename" ||
            choice === "delete"
          ) {
            setMode(choice);
            setDraft("");
            setFrom(names[0] ?? "");
          } else {
            const name = choice.slice("branch:".length);
            if (!canCheckoutLocalBranch(name, repository.branch, names)) return;
            if (repository.changes.length || repository.changesOverflowed)
              setPendingBranch(name);
            else await switchBranch(name);
          }
        }}
      />
      {pendingBranch && !disabled ? (
        <ConfirmDialog
          title="Switch branch with uncommitted changes?"
          description={`Your uncommitted changes will be carried to ${pendingBranch} if Git can preserve them. Commit or stash conflicting changes before switching.`}
          confirmLabel="Switch branch"
          onCancel={() => setPendingBranch(null)}
          onConfirm={() => {
            setPendingBranch(null);
            void switchBranch(pendingBranch);
          }}
        />
      ) : null}
      {checkoutFailed && error?.rootId === repository.rootId ? (
        <MenuPopover
          className="menu-popover--editor"
          id={`${id}-error`}
          role="dialog"
          ariaLabel="Cannot switch branch"
          open
          triggerRef={trigger}
          onDismiss={() => setCheckoutFailed(false)}
        >
          <p role="alert">{error.message}</p>
          <button
            className="menu-item"
            type="button"
            onClick={() => setCheckoutFailed(false)}
          >
            Close
          </button>
        </MenuPopover>
      ) : null}
      {mode ? (
        <MenuPopover
          className="menu-popover--editor"
          ariaLabel={`${mode === "create" ? "New" : mode === "rename" ? "Rename" : "Delete"} branch in ${rootLabel}`}
          id={id}
          role="dialog"
          open
          triggerRef={trigger}
          onDismiss={() => setMode(null)}
          placement="above-start"
          trapFocus
        >
          <form
            className="source-branch-create"
            aria-label={`${mode === "create" ? "New" : mode === "rename" ? "Rename" : "Delete"} branch in ${rootLabel}`}
            onSubmit={async (event) => {
              event.preventDefault();
              if (!valid || disabled) return;
              const success =
                mode === "create"
                  ? await create(repository.rootId, draft)
                  : mode === "rename"
                    ? await rename(repository.rootId, from, draft)
                    : await remove(repository.rootId, from);
              if (success) setMode(null);
            }}
          >
            {mode !== "create" ? (
              <SearchablePicker
                ariaLabel={
                  mode === "rename" ? "Branch to rename" : "Branch to delete"
                }
                disabled={disabled}
                candidates={branchCandidates}
                selectedId={from}
                onSelect={setFrom}
              />
            ) : null}
            {mode !== "delete" ? (
              <input
                disabled={disabled}
                autoFocus
                aria-label={
                  mode === "create" ? "New branch name" : "Renamed branch name"
                }
                placeholder={
                  mode === "create" ? "New branch name" : "Renamed branch name"
                }
                maxLength={255}
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
              />
            ) : null}
            <button type="submit" disabled={disabled || !valid}>
              {mode === "create"
                ? "Create branch"
                : mode === "rename"
                  ? "Rename branch"
                  : "Delete branch"}
            </button>
          </form>
          {error?.rootId === repository.rootId ? (
            <p className="sidebar__error" role="alert">
              {error.message}
            </p>
          ) : null}
        </MenuPopover>
      ) : null}
    </div>
  );
}
