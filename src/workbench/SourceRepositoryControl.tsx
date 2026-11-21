import { useId } from "react";
import { useWorkbenchStore } from "../app/store";
import type { RootRepositoryStatus } from "../domain/workbench";
import { repositoryTransportAction } from "../features/git/primaryAction";
import { canPushRemote, visibleRemotes } from "../features/git/remotes";
import { SearchablePicker } from "../ui/SearchablePicker";
import {
  sourceActionChanges,
  sourceActionCandidates,
  type SourceGroupAction,
} from "../features/git/sourceControlActions";
import { Check, Plus, RefreshCw } from "../ui/leanIcons";
import { RepositoryCommitControl } from "./RepositoryCommitControl";
import { SourceRepositoryMenu } from "./SourceRepositoryMenu";

export function SourceRepositoryControl({
  repository,
  rootLabel,
  controlsDisabled,
  writable,
  showName,
  onGroupAction,
}: {
  repository: RootRepositoryStatus;
  rootLabel: string;
  controlsDisabled: boolean;
  writable: boolean;
  showName: boolean;
  onGroupAction: (action: SourceGroupAction) => void;
}) {
  const formId = useId();
  const reasonId = useId();
  const refresh = useWorkbenchStore((state) => state.refreshRepository);
  const loadInventory = useWorkbenchStore(
    (state) => state.loadRepositoryInventory,
  );
  const pull = useWorkbenchStore((state) => state.pullRepository);
  const push = useWorkbenchStore((state) => state.pushRemote);
  const action = writable ? repositoryTransportAction(repository) : null;
  const transport = action;
  const publishRemotes =
    writable && !repository.upstream && !action
      ? visibleRemotes(repository.remotes).filter((remote) =>
          canPushRemote(repository, remote),
        )
      : [];
  return (
    <>
      <div
        className="source-root__header"
        role="group"
        aria-label={`Changes in ${rootLabel}`}
      >
        <span
          className="source-root__name"
          title={showName ? rootLabel : undefined}
        >
          {showName ? rootLabel : "Changes"}
        </span>
        <div className="source-root__actions">
          <button
            type="submit"
            form={formId}
            className="source-change__action"
            aria-label="Commit changes"
            title="Commit"
            disabled={
              controlsDisabled ||
              !writable ||
              (repository.changes.length === 0 && !repository.mergeInProgress)
            }
          >
            <Check size={14} aria-hidden="true" />
          </button>
          {sourceActionCandidates(repository.changes, "stage").length > 0 ? (
            <button
              type="button"
              className="source-change__action"
              aria-label="Stage All Changes"
              title="Stage All Changes"
              disabled={
                controlsDisabled ||
                !writable ||
                sourceActionChanges(repository, "stage").length === 0
              }
              onClick={() => onGroupAction("stage")}
            >
              <Plus size={14} aria-hidden="true" />
            </button>
          ) : null}
          <button
            type="button"
            className="source-change__action"
            aria-label="Refresh Source Control"
            title="Refresh"
            disabled={controlsDisabled}
            onClick={() => {
              void refresh();
              void loadInventory();
            }}
          >
            <RefreshCw size={14} aria-hidden="true" />
          </button>
          <SourceRepositoryMenu
            repository={repository}
            rootLabel={rootLabel}
            controlsDisabled={controlsDisabled}
            writable={writable}
            onGroupAction={onGroupAction}
          />
        </div>
      </div>
      {writable ? (
        <RepositoryCommitControl
          key={repository.rootId}
          formId={formId}
          repository={repository}
          rootLabel={rootLabel}
          controlsDisabled={controlsDisabled}
        />
      ) : null}
      {transport ? (
        <button
          type="button"
          className="source-root__primary"
          disabled={controlsDisabled || transport.reason !== null}
          title={transport.reason ?? transport.label}
          aria-describedby={transport.reason ? reasonId : undefined}
          onClick={() => {
            if (transport.kind === "pull") void pull(repository.rootId);
            else if (transport.kind === "push" || transport.kind === "publish")
              void push(repository.rootId, transport.remote);
          }}
        >
          {transport.label}
        </button>
      ) : null}
      {transport?.reason ? (
        <p id={reasonId} className="source-root__action-reason">
          {transport.reason}
        </p>
      ) : null}
      {publishRemotes.length > 1 ? (
        <SearchablePicker
          ariaLabel="Publish Branch"
          appearance="plain"
          triggerClassName="source-root__primary"
          triggerLabel="Publish Branch"
          selectedId={null}
          disabled={controlsDisabled}
          candidates={publishRemotes.map((remote) => ({
            id: remote,
            label: remote,
          }))}
          onSelect={(remote) => void push(repository.rootId, remote)}
        />
      ) : null}
    </>
  );
}
