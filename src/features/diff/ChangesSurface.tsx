import { memo, useMemo, useState } from "react";
import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
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
