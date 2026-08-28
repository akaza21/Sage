import { CaseSensitive, FileSearch, LoaderCircle, X } from "lucide-react";
import type { FormEvent, KeyboardEvent } from "react";

import {
  type WorkspaceSearchGateway,
  type WorkspaceSearchLimitReason,
  type WorkspaceSearchMatch,
  type WorkspaceSearchNavigationRequest,
} from "./workspaceSearchDomain";
import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";
import { workspaceSearchDesktop } from "./workspaceSearchDesktop";
import { useWorkspaceSearch } from "./useWorkspaceSearch";
import { searchResultFileName } from "./searchResultCard";
import { FileKindIcon } from "../files/FileKindIcon";

interface WorkspaceSearchViewProps {
  projectId: string | null;
  transitionEpoch: number;
  indexGeneration: number;
  onOpenMatch: (request: WorkspaceSearchNavigationRequest) => void;
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
  indexGeneration,
  onOpenMatch,
  gateway = workspaceSearchDesktop,
}: WorkspaceSearchViewProps) {
  const { state, setQuery, setCaseSensitive, submit, continueSearch, cancel } =
    useWorkspaceSearch({
      projectId,
      transitionEpoch,
      indexGeneration,
      gateway,
    });

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void submit();
  }

  function openMatch(match: WorkspaceSearchMatch) {
    const response = state.response;
    if (!response) return;
    onOpenMatch({
      projectId: response.projectId,
      transitionEpoch: response.transitionEpoch,
      resource: match.resource,
      lineNumber: match.lineNumber,
      columnNumber: match.columnNumber,
      query: response.query,
      caseSensitive: state.caseSensitive,
    });
  }

  function handleMatchKeyDown(
    event: KeyboardEvent<HTMLButtonElement>,
    match: WorkspaceSearchMatch,
  ) {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    openMatch(match);
  }

  const matches = state.pages
    .flatMap((page) => page.matches)
    .slice(
      0,
      WORKSPACE_SEARCH_LIMITS.results *
        WORKSPACE_SEARCH_LIMITS.continuationPages,
    );
  const lastPage = state.pages.at(-1);
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
            Searching the Project...
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
            {state.pages.length < WORKSPACE_SEARCH_LIMITS.continuationPages &&
            lastPage?.continuationToken ? (
              <button type="button" onClick={() => void continueSearch()}>
                Continue search
              </button>
            ) : null}
            {matches.length === 0 ? (
              <p className="workspace-search__empty">
                No matches found for "{state.response.query}".
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
                      onClick={() => openMatch(match)}
                      onKeyDown={(event) => handleMatchKeyDown(event, match)}
                    >
                      <FileKindIcon
                        path={match.resource.relativePath}
                        entryKind="file"
                      />
                      <span className="workspace-search__copy">
                        <span className="workspace-search__path">
                          <strong>
                            {searchResultFileName(
                              match.resource.relativePath,
                            ) || match.resource.relativePath}
                          </strong>
                          <span>{match.resource.relativePath}</span>
                          <span>
                            {rootNames.get(match.resource.rootId) ?? "Root"}
                          </span>
                        </span>
                        <span className="workspace-search__preview">
                          {match.preview || "(empty line)"}
                        </span>
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
