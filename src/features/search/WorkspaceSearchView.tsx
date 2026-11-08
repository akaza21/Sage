import {
  ChevronsUpDown,
  LoaderCircle,
  RefreshCw,
  ChevronDown,
  ChevronRight,
  Replace,
  ReplaceAll,
} from "../../ui/leanIcons";

import { useMemo, useState, type KeyboardEvent } from "react";

import {
  type WorkspaceSearchGateway,
  type WorkspaceSearchLimitReason,
  type WorkspaceSearchMatch,
  type WorkspaceSearchNavigationRequest,
  searchResultFileName,
} from "./workspaceSearchDomain";
import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";
import { workspaceSearchDesktop } from "./workspaceSearchDesktop";
import { useWorkspaceSearch } from "./useWorkspaceSearch";
import { WorkspaceSearchCommandBar } from "./WorkspaceSearchCommandBar";
import { FileKindIcon } from "../files/FileKindIcon";
import {
  workspaceReplaceDesktop,
  type WorkspaceReplaceGateway,
} from "./workspaceReplace";
import { useWorkspaceReplace } from "./useWorkspaceReplace";
import {
  WorkspaceReplaceConfirm,
  WorkspaceReplaceStatus,
} from "./WorkspaceReplaceDialog";
import { highlightPreview } from "./workspaceSearchHighlight";
import "./workspace-replace.css";

interface WorkspaceSearchViewProps {
  projectId: string | null;
  transitionEpoch: number;
  indexGeneration: number;
  onOpenMatch: (request: WorkspaceSearchNavigationRequest) => void;
  gateway?: WorkspaceSearchGateway;
  replaceGateway?: WorkspaceReplaceGateway;
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
  replaceGateway = workspaceReplaceDesktop,
}: WorkspaceSearchViewProps) {
  const {
    state,
    setQuery,
    setCaseSensitive,
    setWholeWord,
    setUseRegex,
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
  const replace = useWorkspaceReplace({
    gateway: replaceGateway,
    search: state,
    onReplaced: () => void refresh(),
  });
  const groupClass = replace.open
    ? "workspace-search__file-group workspace-replace__file-group"
    : "workspace-search__file-group";
  const matchClass = replace.open ? "workspace-replace__match" : undefined;

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
            aria-label="Toggle Replace"
            title="Toggle Replace"
            aria-pressed={replace.open}
            disabled={!projectId}
            onClick={() => replace.setOpen((open) => !open)}
          >
            <Replace size={14} aria-hidden="true" />
          </button>
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
      <WorkspaceSearchCommandBar
        projectId={projectId}
        searching={state.phase === "searching"}
        query={state.query}
        caseSensitive={state.caseSensitive}
        wholeWord={state.wholeWord}
        useRegex={state.useRegex}
        includePatternText={state.includePatternText}
        excludePatternText={state.excludePatternText}
        onQueryChange={setQuery}
        onCaseSensitiveChange={setCaseSensitive}
        onWholeWordChange={setWholeWord}
        onUseRegexChange={setUseRegex}
        replace={
          replace.open
            ? {
                replacement: replace.replacement,
                preserveCase: replace.preserveCase,
                canReplaceAll:
                  state.phase === "results" &&
                  groups.length > 0 &&
                  replace.idle,
                onReplacementChange: replace.setReplacement,
                onPreserveCaseChange: replace.setPreserveCase,
                onReplaceAll: () =>
                  void replace.start(
                    groups.map(([, fileMatches]) => fileMatches[0].resource),
                  ),
              }
            : undefined
        }
        onIncludePatternTextChange={setIncludePatternText}
        onExcludePatternTextChange={setExcludePatternText}
        onSearch={() => void submit()}
        onCancel={cancel}
      />

      <div className="workspace-search__body" aria-live="polite">
        <WorkspaceReplaceStatus replace={replace} />
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
                  const relativePath = first.resource.relativePath;
                  const fileName =
                    searchResultFileName(relativePath) || relativePath;
                  const directory = relativePath
                    .slice(
                      0,
                      Math.max(0, relativePath.length - fileName.length),
                    )
                    .replace(/[\\/]$/, "");
                  const rootName =
                    rootNames.get(first.resource.rootId) ?? "Root";
                  return (
                    <section className={groupClass} key={key}>
                      <button
                        className="workspace-search__file-heading"
                        type="button"
                        aria-expanded={!collapsed}
                        aria-label={`${collapsed ? "Expand" : "Collapse"} results for ${relativePath}`}
                        title={relativePath}
                        onClick={() =>
                          setCollapsedPaths((current) => {
                            const next = new Set(current);
                            if (next.has(key)) next.delete(key);
                            else next.add(key);
                            return next;
                          })
                        }
                      >
                        {collapsed ? (
                          <ChevronRight size={14} aria-hidden="true" />
                        ) : (
                          <ChevronDown size={14} aria-hidden="true" />
                        )}
                        <FileKindIcon path={relativePath} entryKind="file" />
                        <span className="workspace-search__file-identity">
                          <strong>{fileName}</strong>
                          <span>
                            {directory
                              ? `${rootName} › ${directory}`
                              : rootName}
                          </span>
                        </span>
                        <span className="workspace-search__file-count">
                          {fileMatches.length}
                        </span>
                      </button>
                      {replace.open ? (
                        <button
                          className="workspace-replace__action"
                          type="button"
                          aria-label={`Replace all in ${relativePath}`}
                          title="Replace all in this file"
                          disabled={!replace.idle}
                          onClick={() => void replace.start([first.resource])}
                        >
                          <ReplaceAll size={14} aria-hidden="true" />
                        </button>
                      ) : null}
                      {!collapsed
                        ? fileMatches.map((match, index) => (
                            <div
                              key={`${match.lineNumber}:${match.columnNumber}:${index}`}
                              role="listitem"
                              className={matchClass}
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
                                <span className="workspace-search__preview">
                                  {highlightPreview(
                                    match.preview,
                                    state.response?.query ?? "",
                                    state.caseSensitive,
                                    state.wholeWord,
                                    fileMatches
                                      .slice(0, index)
                                      .filter(
                                        (candidate) =>
                                          candidate.lineNumber ===
                                          match.lineNumber,
                                      ).length,
                                    state.useRegex,
                                  )}
                                </span>
                                <span className="workspace-search__position">
                                  {match.lineNumber}:{match.columnNumber}
                                </span>
                              </button>
                              {replace.open ? (
                                <button
                                  className="workspace-replace__action"
                                  type="button"
                                  aria-label={`Replace the match at line ${match.lineNumber}, column ${match.columnNumber} in ${match.resource.relativePath}`}
                                  title="Replace this match"
                                  disabled={!replace.idle}
                                  onClick={() =>
                                    void replace.start([match.resource], {
                                      lineNumber: match.lineNumber,
                                      columnNumber: match.columnNumber,
                                    })
                                  }
                                >
                                  <Replace size={14} aria-hidden="true" />
                                </button>
                              ) : null}
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
      <WorkspaceReplaceConfirm
        replace={replace}
        rootName={(rootId) => rootNames.get(rootId) ?? "Root"}
      />
    </div>
  );
}
