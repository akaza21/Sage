import { useMemo, useState, type FormEvent } from "react";

import {
  canCheckoutLocalBranch,
  canCreateLocalBranch,
  canDeleteLocalBranch,
  canRenameLocalBranch,
  isCurrentLocalBranch,
} from "../features/git/localBranches";
import { SearchablePicker } from "../ui/SearchablePicker";

interface Props {
  rootLabel: string;
  branch: string | null;
  branches?: string[];
  truncated?: boolean;
  checkoutDisabled?: boolean;
  createDisabled?: boolean;
  deleteDisabled?: boolean;
  renameDisabled?: boolean;
  onCheckout?: (name: string) => void;
  onCreate?: (name: string) => void;
  onDelete?: (name: string) => void;
  onRename?: (from: string, to: string) => void;
}

const EMPTY_BRANCHES: readonly string[] = [];

export function SourceBranchList({
  rootLabel,
  branch,
  branches,
  truncated = false,
  checkoutDisabled = false,
  createDisabled = false,
  deleteDisabled = false,
  renameDisabled = false,
  onCheckout,
  onCreate,
  onDelete,
  onRename,
}: Props) {
  const [draft, setDraft] = useState("");
  const [renameFrom, setRenameFrom] = useState("");
  const [renameTo, setRenameTo] = useState("");
  const names = branches ?? EMPTY_BRANCHES;
  const branchCandidates = useMemo(
    () => names.map((name) => ({ id: name, label: name })),
    [names],
  );
  const selectedFrom = names.includes(renameFrom)
    ? renameFrom
    : (names[0] ?? "");
  const canSubmit =
    onCreate !== undefined && canCreateLocalBranch(draft, names, truncated);
  const canRename =
    onRename !== undefined &&
    canRenameLocalBranch(selectedFrom, renameTo, names, truncated);
  if (
    names.length === 0 &&
    !truncated &&
    onCreate === undefined &&
    onRename === undefined
  ) {
    return null;
  }
  const submitCreate = (event: FormEvent) => {
    event.preventDefault();
    if (!canSubmit || createDisabled || onCreate === undefined) {
      return;
    }
    onCreate(draft);
    setDraft("");
  };
  const submitRename = (event: FormEvent) => {
    event.preventDefault();
    if (!canRename || renameDisabled || onRename === undefined) {
      return;
    }
    onRename(selectedFrom, renameTo);
    setRenameTo("");
  };
  return (
    <>
      {names.length > 0 || onCreate !== undefined || onRename !== undefined ? (
        <>
          <p className="source-group-label">Branches</p>
          {names.length > 0 ? (
            <ul
              className="source-branches"
              aria-label={`Local branches in ${rootLabel}`}
            >
              {names.map((name) => {
                const current = isCurrentLocalBranch(name, branch);
                const canCheckout =
                  onCheckout !== undefined &&
                  canCheckoutLocalBranch(name, branch, names);
                const canDelete =
                  onDelete !== undefined &&
                  canDeleteLocalBranch(name, branch, names, truncated);
                return (
                  <li
                    key={name}
                    className={
                      current
                        ? "source-branch source-branch--current"
                        : "source-branch"
                    }
                  >
                    {current ? (
                      `${name} (current)`
                    ) : (
                      <span className="source-branch__row">
                        {canCheckout ? (
                          <button
                            type="button"
                            className="source-branch__switch"
                            disabled={checkoutDisabled}
                            onClick={() => onCheckout(name)}
                          >
                            Switch to {name}
                          </button>
                        ) : (
                          name
                        )}
                        {canDelete ? (
                          <button
                            type="button"
                            className="source-branch__delete"
                            disabled={deleteDisabled}
                            onClick={() => onDelete(name)}
                          >
                            Delete {name}
                          </button>
                        ) : null}
                      </span>
                    )}
                  </li>
                );
              })}
            </ul>
          ) : null}
          {onCreate !== undefined ? (
            <form
              className="source-branch-create"
              aria-label={`New branch in ${rootLabel}`}
              onSubmit={submitCreate}
            >
              <label className="source-branch-create__label">
                <span className="sr-only">New branch name</span>
                <input
                  type="text"
                  value={draft}
                  maxLength={255}
                  placeholder="New branch"
                  aria-label="New branch name"
                  disabled={createDisabled}
                  onChange={(event) => setDraft(event.target.value)}
                />
              </label>
              <button type="submit" disabled={createDisabled || !canSubmit}>
                Create branch
              </button>
            </form>
          ) : null}
          {onRename !== undefined && names.length > 0 ? (
            <form
              className="source-branch-create"
              aria-label={`Rename branch in ${rootLabel}`}
              onSubmit={submitRename}
            >
              <label className="source-branch-create__label">
                <span className="sr-only">Branch to rename</span>
                <SearchablePicker
                  ariaLabel="Branch to rename"
                  disabled={renameDisabled}
                  candidates={branchCandidates}
                  onSelect={setRenameFrom}
                  selectedId={selectedFrom}
                />
              </label>
              <label className="source-branch-create__label">
                <span className="sr-only">Renamed branch name</span>
                <input
                  type="text"
                  value={renameTo}
                  maxLength={255}
                  placeholder="Renamed branch"
                  aria-label="Renamed branch name"
                  disabled={renameDisabled}
                  onChange={(event) => setRenameTo(event.target.value)}
                />
              </label>
              <button type="submit" disabled={renameDisabled || !canRename}>
                Rename branch
              </button>
            </form>
          ) : null}
        </>
      ) : null}
      {truncated ? (
        <p className="tree__limit">Branch list capped at 64 names.</p>
      ) : null}
    </>
  );
}
