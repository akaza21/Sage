import { CaseSensitive, FileSearch, LoaderCircle, X } from "lucide-react";
import type { FormEvent } from "react";

import type { ResourceRef } from "../../domain/workbench";
import {
  MAX_WORKSPACE_SEARCH_RESULTS,
  type WorkspaceSearchGateway,
  type WorkspaceSearchLimitReason,
} from "./workspaceSearchDomain";
import { workspaceSearchDesktop } from "./workspaceSearchDesktop";
import { useWorkspaceSearch } from "./useWorkspaceSearch";
import "./workspace-search.css";

interface WorkspaceSearchViewProps {
  projectId: string | null;
  transitionEpoch: number;
  onOpenResource: (resource: ResourceRef) => void;
  gateway?: WorkspaceSearchGateway;
}

const limitMessages: Record<WorkspaceSearchLimitReason, string> = {
  resultLimit: "Only the first 500 matches are shown.",
  fileLimit: "The scan reached its file-count limit.",
  fileSizeLimit: "Some files exceeded the per-file size limit.",
  byteLimit: "The scan reached its aggregate byte limit.",
  entryLimit: "The scan reached its filesystem-entry limit.",
  depthLimit: "Folders below the maximum search depth were skipped.",
  timeLimit: "The scan reached its time limit.",
};

export function WorkspaceSearchView({
  projectId,
  transitionEpoch,
  onOpenResource,
  gateway = workspaceSearchDesktop,
}: WorkspaceSearchViewProps) {
  const { state, setQuery, setCaseSensitive, submit, cancel } =
    useWorkspaceSearch({ projectId, transitionEpoch, gateway });

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void submit();
  }

  const matches =
    state.response?.matches.slice(0, MAX_WORKSPACE_SEARCH_RESULTS) ?? [];
  const rootNames = new Map(
    state.response?.roots.map((root) => [root.rootId, root.rootName]) ?? [],
  );
  const rootIssues =
    state.response?.roots.filter(
      (root) => root.state !== "complete" || root.message !== null,
    ) ?? [];

  return (
    <div className="workspace-search">
      <div className="sidebar__heading">Search</div>
      <form className="workspace-search__form" onSubmit={handleSubmit}>
        <label className="workspace-search__input">
          <FileSearch size={14} aria-hidden="true" />
          <span className="sr-only">Search files</span>
          <input
            type="search"
            aria-label="Search files"
            maxLength={512}
            placeholder="Search files"
            value={state.query}
            disabled={!projectId}
            onChange={(event) => setQuery(event.target.value)}
          />
          {state.query ? (
            <button
              type="button"
              aria-label="Clear Search"
              onClick={() => setQuery("")}
            >
              <X size={13} aria-hidden="true" />
            </button>
          ) : null}
        </label>
        <div className="workspace-search__controls">
          <label className="workspace-search__case">
            <input
              type="checkbox"
              checked={state.caseSensitive}
              disabled={!projectId || state.phase === "searching"}
              onChange={(event) => setCaseSensitive(event.target.checked)}
            />
            <CaseSensitive size={15} aria-hidden="true" />
            Match case
          </label>
          {state.phase === "searching" ? (
            <button type="button" onClick={cancel}>
              Cancel
            </button>
          ) : (
            <button type="submit" disabled={!projectId || !state.query.trim()}>
              Search
            </button>
          )}
        </div>
      </form>

      <div className="workspace-search__body" aria-live="polite">
        {!projectId ? (
          <p className="workspace-search__empty">
            Open a Project to search its files.
          </p>
        ) : null}
        {state.phase === "idle" && projectId ? (
          <p className="workspace-search__empty">
            Search all roots in the active Project.
          </p>
        ) : null}
        {state.phase === "searching" ? (
          <p className="workspace-search__empty workspace-search__progress">
            <LoaderCircle size={14} aria-hidden="true" />
            Searching the Project…
          </p>
        ) : null}
        {state.phase === "error" ? (
          <p className="workspace-search__error">{state.error}</p>
        ) : null}
        {state.phase === "results" && state.response ? (
          <>
            <div className="workspace-search__summary">
              <span>
                {matches.length} match{matches.length === 1 ? "" : "es"}
              </span>
              <span>{state.response.elapsedMs} ms</span>
            </div>
            {state.response.limitReasons.map((reason) => (
              <p className="workspace-search__notice" key={reason}>
                {limitMessages[reason]}
              </p>
            ))}
            {rootIssues.map((root) => (
              <p className="workspace-search__notice" key={root.rootId}>
                {root.rootName}: {root.message ?? "Search was partial."}
              </p>
            ))}
            {matches.length === 0 ? (
              <p className="workspace-search__empty">
                No matches found for “{state.response.query}”.
              </p>
            ) : (
              <div className="workspace-search__results" role="list">
                {matches.map((match, index) => (
                  <div
                    key={`${match.resource.rootId}:${match.resource.relativePath}:${match.lineNumber}:${match.columnNumber}:${index}`}
                    role="listitem"
                  >
                    <button
                      className="workspace-search__result"
                      type="button"
                      aria-label={`Open ${match.resource.relativePath} at line ${match.lineNumber}, column ${match.columnNumber}`}
                      onClick={() => onOpenResource(match.resource)}
                    >
                      <span className="workspace-search__path">
                        <strong>{match.resource.relativePath}</strong>
                        <span>
                          {rootNames.get(match.resource.rootId) ?? "Root"}
                        </span>
                      </span>
                      <span className="workspace-search__preview">
                        {match.preview || "(empty line)"}
                      </span>
                      <span className="workspace-search__position">
                        {match.lineNumber}:{match.columnNumber}
                      </span>
                    </button>
                  </div>
                ))}
              </div>
            )}
          </>
        ) : null}
      </div>
    </div>
  );
}
