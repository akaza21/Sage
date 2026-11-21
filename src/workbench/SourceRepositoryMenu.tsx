import { lazy, Suspense, useId, useRef, useState } from "react";
import { useWorkbenchStore } from "../app/store";
import type { RootRepositoryStatus } from "../domain/workbench";
import {
  canFetchRemote,
  canPushRemote,
  visibleRemotes,
} from "../features/git/remotes";
import {
  sourceActionChanges,
  sourceActionCandidates,
  type SourceGroupAction,
} from "../features/git/sourceControlActions";
import { Ellipsis } from "../ui/leanIcons";
import { MenuPopover } from "../ui/MenuPopover";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { SourceCommitList } from "./SourceCommitList";
const SourceBranchList = lazy(() =>
  import("./StatusBarDetails").then((module) => ({
    default: module.SourceBranchList,
  })),
);

export function SourceRepositoryMenu({
  repository,
  rootLabel,
  controlsDisabled,
  writable,
  onGroupAction,
}: {
  repository: RootRepositoryStatus;
  rootLabel: string;
  controlsDisabled: boolean;
  writable: boolean;
  onGroupAction: (action: SourceGroupAction) => void;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [discard, setDiscard] = useState(false);
  const [commits, setCommits] = useState(false);
  const fetch = useWorkbenchStore((state) => state.fetchRemote);
  const push = useWorkbenchStore((state) => state.pushRemote);
  const pull = useWorkbenchStore((state) => state.pullRepository);
  return (
    <>
      <button
        ref={trigger}
        type="button"
        className="source-change__action"
        aria-label="More Source Control Actions"
        title="More Actions…"
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={controlsDisabled}
        onClick={() => {
          setCommits(false);
          setOpen(!open);
        }}
      >
        <Ellipsis size={14} aria-hidden="true" />
      </button>
      {open ? (
        <MenuPopover
          className="menu-popover--editor"
          id={id}
          ariaLabel="Source Control actions"
          open
          triggerRef={trigger}
          onDismiss={() => setOpen(false)}
          role={commits ? "dialog" : "menu"}
        >
          {commits ? (
            <SourceCommitList
              rootLabel={rootLabel}
              commits={repository.recentCommits}
              truncated={repository.recentCommitsTruncated}
            />
          ) : (
            <>
              {writable ? (
                <Suspense fallback={null}>
                  <SourceBranchList
                    repository={repository}
                    rootLabel={rootLabel}
                    disabled={controlsDisabled}
                    triggerLabel="Branches…"
                    triggerRole="menuitem"
                  />
                </Suspense>
              ) : null}
              <button
                className="menu-item"
                role="menuitem"
                type="button"
                disabled={controlsDisabled || !writable || !repository.upstream}
                onClick={() => {
                  setOpen(false);
                  void pull(repository.rootId);
                }}
              >
                Pull
              </button>
              {visibleRemotes(repository.remotes).map((remote) => (
                <div key={remote}>
                  <button
                    className="menu-item"
                    role="menuitem"
                    type="button"
                    disabled={
                      controlsDisabled ||
                      !writable ||
                      !canFetchRemote(repository, remote)
                    }
                    onClick={() => {
                      setOpen(false);
                      void fetch(repository.rootId, remote);
                    }}
                  >
                    Fetch {remote}
                  </button>
                  <button
                    className="menu-item"
                    role="menuitem"
                    type="button"
                    disabled={
                      controlsDisabled ||
                      !writable ||
                      !canPushRemote(repository, remote)
                    }
                    onClick={() => {
                      setOpen(false);
                      void push(repository.rootId, remote);
                    }}
                  >
                    Push {remote}
                  </button>
                </div>
              ))}
              {sourceActionCandidates(repository.changes, "discard").length >
              0 ? (
                <button
                  className="menu-item"
                  role="menuitem"
                  type="button"
                  disabled={
                    !writable ||
                    sourceActionChanges(repository, "discard").length === 0
                  }
                  onClick={() => {
                    setOpen(false);
                    setDiscard(true);
                  }}
                >
                  Discard All Changes…
                </button>
              ) : null}
              <button
                className="menu-item"
                role="menuitem"
                type="button"
                onClick={() => setCommits(true)}
              >
                Recent commits…
              </button>
            </>
          )}
        </MenuPopover>
      ) : null}
      {discard ? (
        <ConfirmDialog
          title="Discard all changes?"
          description="Discard all safe unstaged changes in this repository? This cannot be undone. Staged and untracked files are preserved."
          confirmLabel="Discard All Changes"
          onCancel={() => setDiscard(false)}
          onConfirm={() => {
            setDiscard(false);
            onGroupAction("discard");
          }}
        />
      ) : null}
    </>
  );
}
