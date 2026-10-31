import { memo, useMemo, useState } from "react";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { openAgentArtifact } from "../../app/openAgentArtifact";
import { openAgentSnapshotSource } from "../../app/agentDiffSource";
import { resourceTabUpdate } from "../../app/resourceTabs";
import { useAgentArtifactAdmissionStore } from "../../app/agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "../../app/agentArtifactFollowStore";
import { useAgentChangesState } from "../../workbench/agentChangesState";
import { useAgentChangesScopes } from "../../workbench/agentChangesStateScopes";
import { sameResource } from "../../domain/resourceIdentity";
import type { ChangesEntry } from "./agentChanges";
import type { AcpChatPage } from "../../domain/acpProjection";
import { resourceKey } from "../../domain/resourceIdentity";
import { isTauriRuntime } from "../../ports/desktopRuntime";
import { useTurnWorkspaceChanges } from "../chat/acp/turnWorkspaceBridge";
import { ChangesFileDiff } from "./ChangesFileDiff";
import { projectChanges, type ChangesScope } from "./agentChanges";
import "./changes.css";

export const ChangesSurface = memo(function ChangesSurface({
  page: suppliedPage = null,
  chatId = null,
}: {
  page?: AcpChatPage | null;
  chatId?: string | null;
}) {
  const [page, setPage] = useState(
    () =>
      suppliedPage ??
      (chatId ? acpProjectionStoreFor(chatId).getState().page : null),
  );
  const repositories = useWorkbenchStore((state) => state.repositories);
  const refreshRepository = useWorkbenchStore(
    (state) => state.refreshRepository,
  );
  const [scope, setScope] = useState<ChangesScope>("lastTurn");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState("");
  const [revision, setRevision] = useState(0);
  async function openEntry(entry: ChangesEntry, source: boolean) {
    setError("");
    const before = useWorkbenchStore.getState();
    const changesOwner = useAgentChangesState.getState().owner;
    if (
      before.projectTransitioning ||
      (before.project?.id !== page?.projectId && scope === "lastTurn")
    )
      return;
    try {
      if (entry.diffRef) {
        if (
          !(source
            ? await openAgentSnapshotSource(entry.diffRef)
            : await openAgentArtifact({
                owner: entry.diffRef,
                target: "diff",
                resource: entry.resource,
                diffRef: entry.diffRef,
              }))
        )
          setError(
            "This saved change is no longer available. Refresh changes.",
          );
        return;
      }
      if (source) await before.openFile(entry.resource);
      else {
        if (
          before.fileCreateInFlight ||
          before.folderCreateInFlight ||
          before.fileRenameInFlight ||
          before.resourceTrashInFlight
        )
          return;
        await before.flushActiveEditor();
        const current = useWorkbenchStore.getState();
        if (
          current.projectTransitioning ||
          current.projectGeneration !== before.projectGeneration ||
          current.project?.id !== before.project?.id ||
          !current.project?.roots.some(
            (root) => root.id === entry.resource.rootId,
          ) ||
          useAgentChangesState.getState().owner !== changesOwner
        )
          return;
        useWorkbenchStore.setState(
          resourceTabUpdate(current, "diff", entry.resource),
        );
        const retained = useWorkbenchStore.getState().tabs;
        for (const old of current.tabs)
          if (!retained.some((tab) => tab.id === old.id)) {
            useAgentArtifactAdmissionStore.getState().forget(old.id);
            useAgentArtifactFollowStore.getState().forgetTab(old.id);
          }
      }
      const state = useWorkbenchStore.getState();
      if (
        state.projectTransitioning ||
        state.projectGeneration !== before.projectGeneration ||
        state.project?.id !== before.project?.id ||
        useAgentChangesState.getState().owner !== changesOwner
      )
        return;
      const tab = state.tabs.find(
        (candidate) =>
          candidate.id === state.activeTabId &&
          candidate.kind === (source ? "file" : "diff") &&
          sameResource(candidate.resource, entry.resource) &&
          !candidate.diffRef,
      );
      if (!tab) return;
      if (!source && entry.gitScope) {
        useAgentChangesScopes
          .getState()
          .setGitScope(tab, entry.gitScope, state.projectGeneration);
        useWorkbenchStore.setState({ activeEditor: null });
        void state.revealFile(entry.resource);
      }
      const owner = useAgentChangesState.getState().owner;
      if (owner?.sessionId && owner.projectId === state.project?.id) {
        const livePage = acpProjectionStoreFor(owner.chatId).getState().page;
        if (
          !livePage ||
          livePage.projectId !== owner.projectId ||
          livePage.chatId !== owner.chatId ||
          livePage.currentSessionId !== owner.sessionId
        )
          return;
        const displayOwner = {
          projectId: livePage.projectId,
          chatId: livePage.chatId,
          sessionId: livePage.currentSessionId,
        };
        useAgentArtifactAdmissionStore
          .getState()
          .admit({ owner: displayOwner, tabId: tab.id });
        useAgentArtifactFollowStore
          .getState()
          .pin(displayOwner, { kind: "tab", tabId: tab.id });
      }
      useAgentChangesState.getState().select(false);
    } catch (cause) {
      setError(String(cause).slice(0, 512));
    }
  }
  const workspaceChanges = useTurnWorkspaceChanges(
    page,
    scope === "lastTurn" && isTauriRuntime(),
    revision,
  );
  const scopedRepositories = scope === "lastTurn" ? null : repositories;
  const result = useMemo(
    () =>
      projectChanges(
        scope,
        page,
        scopedRepositories,
        workspaceChanges.changes,
        workspaceChanges.incompleteTurnIds,
      ),
    [scope, page, scopedRepositories, workspaceChanges],
  );
  const gitAvailable = repositories.roots.some(
    (root) => root.state === "repository",
  );
  let totals =
    !result.truncated && result.entries.length > 0
      ? { additions: 0, deletions: 0 }
      : null;
  if (totals) {
    for (const entry of result.entries) {
      if (entry.additions === null || entry.deletions === null) {
        totals = null;
        break;
      }
      totals.additions += entry.additions;
      totals.deletions += entry.deletions;
    }
  }
  return (
    <section className="changes-surface" aria-label="Chat changes">
      <div className="changes-surface__toolbar">
        <select
          aria-label="Changes scope"
          value={scope}
          onChange={(event) => {
            setScope(event.target.value as ChangesScope);
            setExpanded(null);
          }}
        >
          <option value="lastTurn">Last turn</option>
          <option value="unstaged" disabled={!gitAvailable}>
            Unstaged{!gitAvailable ? " — Git unavailable" : ""}
          </option>
          <option value="staged" disabled={!gitAvailable}>
            Staged{!gitAvailable ? " — Git unavailable" : ""}
          </option>
        </select>
        {totals ? (
          <span className="diff-header__counts">
            <span>+{totals.additions}</span>
            <span>−{totals.deletions}</span>
          </span>
        ) : null}
        {
          <button
            className="button button--compact"
            type="button"
            disabled={refreshing}
            onClick={() => {
              if (scope === "lastTurn") {
                setRevision((value) => value + 1);
                setPage(
                  suppliedPage ??
                    (chatId
                      ? acpProjectionStoreFor(chatId).getState().page
                      : null),
                );
                setExpanded(null);
                return;
              }
              setRefreshing(true);
              setError("");
              void refreshRepository()
                .catch((cause: unknown) =>
                  setError(String(cause).slice(0, 512)),
                )
                .finally(() => {
                  setRevision((value) => value + 1);
                  setRefreshing(false);
                  setExpanded(null);
                });
            }}
          >
            Refresh changes
          </button>
        }
      </div>
      {scope === "lastTurn" ? (
        <p className="changes-surface__hint">
          Last turn snapshot · Refresh to update
        </p>
      ) : (
        <p className="changes-surface__hint">
          Project Git changes · not attributed to this Chat
        </p>
      )}
      {error ? <p role="alert">{error}</p> : null}
      {result.entries.length === 0 && !result.truncated ? (
        <p className="resource-state">No changes in this scope.</p>
      ) : null}
      {result.truncated ? (
        <p role="status">Showing a bounded partial list of changes.</p>
      ) : null}
      {result.entries.map((entry, index) => {
        const key = resourceKey(entry.resource);
        const selected = expanded === key || (expanded === null && index === 0);
        return (
          <section
            key={key}
            aria-label={`Changes ${entry.resource.relativePath}`}
          >
            <button
              type="button"
              className="changes-surface__file"
              aria-expanded={selected}
              onClick={() => setExpanded(selected ? "" : key)}
            >
              <span title={entry.resource.relativePath}>
                {entry.resource.relativePath}
              </span>
              <span className="diff-header__counts">
                <span>
                  {entry.additions === null ? "" : `+${entry.additions}`}
                </span>
                <span>
                  {entry.deletions === null ? "" : `−${entry.deletions}`}
                </span>
              </span>
            </button>
            <div className="changes-surface__file-actions">
              <button
                className="button button--compact"
                type="button"
                disabled={Boolean(entry.unavailable)}
                onClick={() => void openEntry(entry, false)}
              >
                Open diff
              </button>
              <button
                className="button button--compact"
                type="button"
                onClick={() => void openEntry(entry, true)}
              >
                View live source
              </button>
            </div>
            {selected ? (
              entry.unavailable ? (
                <p className="resource-state">{entry.unavailable}</p>
              ) : (
                <ChangesFileDiff
                  key={`${scope}:${key}:${revision}`}
                  entry={entry}
                />
              )
            ) : null}
          </section>
        );
      })}
    </section>
  );
});
