import {
  CaseSensitive,
  ChevronsUpDown,
  FileSearch,
  LoaderCircle,
  RefreshCw,
  X,
} from "lucide-react";
import { useMemo, useState, type FormEvent, type KeyboardEvent } from "react";

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

function highlightPreview(
  preview: string,
  query: string,
  caseSensitive: boolean,
) {
  const source = preview || "(empty line)";
  if (!query) return source;
  const index = caseSensitive
    ? source.indexOf(query)
    : source.toLocaleLowerCase().indexOf(query.toLocaleLowerCase());
  if (index < 0) return source;
  return (
    <>
      {source.slice(0, index)}
      <mark>{source.slice(index, index + query.length)}</mark>
      {source.slice(index + query.length)}
    </>
  );
}

export function WorkspaceSearchView({
  projectId,
  transitionEpoch,
  indexGeneration,
  onOpenMatch,
  gateway = workspaceSearchDesktop,
}: WorkspaceSearchViewProps) {
  const {
    state,
    setQuery,
    setCaseSensitive,
    setWholeWord,
    setIncludePatternText,
    setExcludePatternText,
    submit,
    continueSearch,
    cancel,
    refresh,
  } = useWorkspaceSearch({
    projectId,
    transitionEpoch,
    indexGeneration,
    gateway,
  });
  const [collapsedPaths, setCollapsedPaths] = useState<ReadonlySet<string>>(
    () => new Set(),
  );

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
  const groups = useMemo(() => {
    const result = new Map<string, WorkspaceSearchMatch[]>();
    for (const match of matches) {
      const key = `${match.resource.rootId}\0${match.resource.relativePath}`;
      result.set(key, [...(result.get(key) ?? []), match]);
    }
    return [...result.entries()];
  }, [matches]);
  const allCollapsed =
    groups.length > 0 && groups.every(([key]) => collapsedPaths.has(key));

  return (
    <div className="workspace-search">
      <div className="sidebar__heading">
        <span>Search</span>
        <span className="sidebar__heading-actions">
          <button
            type="button"
            aria-label="Refresh Search"
            title="Refresh Search"
            disabled={state.phase === "searching" || !state.query.trim()}
            onClick={() => void refresh()}
          >
            <RefreshCw size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label={
              allCollapsed ? "Expand Search results" : "Collapse Search results"
            }
            disabled={groups.length === 0}
            onClick={() =>
              setCollapsedPaths(
                allCollapsed ? new Set() : new Set(groups.map(([key]) => key)),
              )
            }
          >
            <ChevronsUpDown size={14} aria-hidden="true" />
          </button>
        </span>
      </div>
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
          <label className="workspace-search__case">
            <input
              type="checkbox"
              aria-label="Match whole word"
              checked={state.wholeWord}
              disabled={!projectId || state.phase === "searching"}
              onChange={(event) => setWholeWord(event.target.checked)}
            />
            <span aria-hidden="true">ab</span>
            Match whole word
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
        <div className="workspace-search__filters">
          <label>
            <span>Files to include</span>
            <input
              type="text"
              aria-label="Files to include"
              placeholder="e.g. src/**/*.ts, tests/**/*.ts"
              value={state.includePatternText}
              disabled={!projectId || state.phase === "searching"}
              onChange={(event) => setIncludePatternText(event.target.value)}
            />
          </label>
          <label>
            <span>Files to exclude</span>
            <input
              type="text"
              aria-label="Files to exclude"
              placeholder="e.g. **/generated/**"
              value={state.excludePatternText}
              disabled={!projectId || state.phase === "searching"}
              onChange={(event) => setExcludePatternText(event.target.value)}
            />
          </label>
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
                {matches.length} match{matches.length === 1 ? "" : "es"} in{" "}
                {groups.length} file{groups.length === 1 ? "" : "s"}
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
                {groups.map(([key, fileMatches]) => {
                  const first = fileMatches[0];
                  const collapsed = collapsedPaths.has(key);
                  return (
                    <section className="workspace-search__file-group" key={key}>
                      <button
                        className="workspace-search__file-heading"
                        type="button"
                        aria-expanded={!collapsed}
                        aria-label="Toggle result group"
                        aria-description={first.resource.relativePath}
                        title={first.resource.relativePath}
                        onClick={() =>
                          setCollapsedPaths((current) => {
                            const next = new Set(current);
                            if (next.has(key)) next.delete(key);
                            else next.add(key);
                            return next;
                          })
                        }
                      >
                        <FileKindIcon
                          path={first.resource.relativePath}
                          entryKind="file"
                        />
                        <span>{first.resource.relativePath}</span>
                        <span>{fileMatches.length}</span>
                      </button>
                      {!collapsed
                        ? fileMatches.map((match, index) => (
                            <div
                              key={`${match.lineNumber}:${match.columnNumber}:${index}`}
                              role="listitem"
                            >
                              <button
                                className="workspace-search__result"
                                type="button"
                                aria-label={`Open ${match.resource.relativePath} at line ${match.lineNumber}, column ${match.columnNumber}`}
                                onClick={() => openMatch(match)}
                                onKeyDown={(event) =>
                                  handleMatchKeyDown(event, match)
                                }
                              >
                                <FileKindIcon
                                  path={match.resource.relativePath}
                                  entryKind="file"
                                />
                                <span className="workspace-search__copy">
                                  <span
                                    className="workspace-search__path"
                                    title={match.resource.relativePath}
                                  >
                                    <strong>
                                      {searchResultFileName(
                                        match.resource.relativePath,
                                      ) || match.resource.relativePath}
                                    </strong>
                                    <span>
                                      {rootNames.get(match.resource.rootId) ??
                                        "Root"}
                                    </span>
                                  </span>
                                  <span className="workspace-search__preview">
                                    {highlightPreview(
                                      match.preview,
                                      state.response?.query ?? "",
                                      state.caseSensitive,
                                    )}
                                  </span>
                                </span>
                                <span className="workspace-search__position">
                                  {match.lineNumber}:{match.columnNumber}
                                </span>
                              </button>
                            </div>
                          ))
                        : null}
                    </section>
                  );
                })}
              </div>
            )}
          </>
        ) : null}
      </div>
    </div>
  );
}
