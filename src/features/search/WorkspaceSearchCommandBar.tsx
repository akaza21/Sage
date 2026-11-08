import {
  CaseSensitive,
  Square,
  ChevronDown,
  ChevronRight,
  Regex,
  ReplaceAll,
  X,
} from "../../ui/leanIcons";

import { useState, type FormEvent } from "react";

interface WorkspaceSearchCommandBarProps {
  projectId: string | null;
  searching: boolean;
  query: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  useRegex: boolean;
  includePatternText: string;
  excludePatternText: string;
  onQueryChange: (value: string) => void;
  onCaseSensitiveChange: (value: boolean) => void;
  onWholeWordChange: (value: boolean) => void;
  onUseRegexChange: (value: boolean) => void;
  /** The Replace row, shown once the user asks for Replace. */
  replace?: {
    replacement: string;
    preserveCase: boolean;
    /** Replace all is off until there are results to replace. */
    canReplaceAll: boolean;
    onReplacementChange: (value: string) => void;
    onPreserveCaseChange: (value: boolean) => void;
    onReplaceAll: () => void;
  };
  onIncludePatternTextChange: (value: string) => void;
  onExcludePatternTextChange: (value: string) => void;
  onSearch: () => void;
  onCancel: () => void;
}

export function WorkspaceSearchCommandBar({
  projectId,
  searching,
  query,
  caseSensitive,
  wholeWord,
  useRegex,
  includePatternText,
  excludePatternText,
  onQueryChange,
  onCaseSensitiveChange,
  onWholeWordChange,
  onUseRegexChange,
  replace,
  onIncludePatternTextChange,
  onExcludePatternTextChange,
  onSearch,
  onCancel,
}: WorkspaceSearchCommandBarProps) {
  const [detailsOpen, setDetailsOpen] = useState(false);

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    onSearch();
  }

  return (
    <form className="workspace-search__form" onSubmit={handleSubmit}>
      <div className="workspace-search__query-row">
        <button
          className="workspace-search__details-toggle"
          type="button"
          aria-label={
            detailsOpen ? "Hide Search filters" : "Show Search filters"
          }
          title={detailsOpen ? "Hide Search filters" : "Show Search filters"}
          aria-expanded={detailsOpen}
          onClick={() => setDetailsOpen((open) => !open)}
        >
          {detailsOpen ? (
            <ChevronDown size={16} aria-hidden="true" />
          ) : (
            <ChevronRight size={16} aria-hidden="true" />
          )}
        </button>
        <div className="workspace-search__input">
          <input
            type="search"
            aria-label="Search text"
            maxLength={512}
            placeholder="Search"
            value={query}
            disabled={!projectId}
            onChange={(event) => onQueryChange(event.target.value)}
          />
          <span className="workspace-search__query-actions">
            <button
              type="button"
              aria-label="Match case"
              title="Match case"
              aria-pressed={caseSensitive}
              disabled={!projectId || searching}
              onClick={() => onCaseSensitiveChange(!caseSensitive)}
            >
              <CaseSensitive size={16} aria-hidden="true" />
            </button>
            <button
              type="button"
              aria-label="Match whole word"
              title="Match whole word"
              aria-pressed={wholeWord}
              disabled={!projectId || searching}
              onClick={() => onWholeWordChange(!wholeWord)}
            >
              <span aria-hidden="true">ab</span>
            </button>
            <button
              type="button"
              aria-label="Use regular expression"
              title="Use regular expression"
              aria-pressed={useRegex}
              disabled={!projectId || searching}
              onClick={() => onUseRegexChange(!useRegex)}
            >
              <Regex size={16} aria-hidden="true" />
            </button>
            {searching ? (
              <button
                type="button"
                aria-label="Cancel Search"
                title="Cancel Search"
                onClick={onCancel}
              >
                <Square size={10} fill="currentColor" aria-hidden="true" />
              </button>
            ) : query ? (
              <button
                type="button"
                aria-label="Clear Search"
                title="Clear Search"
                onClick={() => onQueryChange("")}
              >
                <X size={14} aria-hidden="true" />
              </button>
            ) : null}
          </span>
        </div>
      </div>
      {replace ? (
        <div className="workspace-search__query-row">
          <span aria-hidden="true" />
          <div className="workspace-search__input">
            <input
              type="text"
              aria-label="Replace text"
              maxLength={2048}
              placeholder={useRegex ? "Replace ($1 for a group)" : "Replace"}
              value={replace.replacement}
              disabled={!projectId}
              onChange={(event) =>
                replace.onReplacementChange(event.target.value)
              }
            />
            <span className="workspace-search__query-actions">
              <button
                type="button"
                aria-label="Preserve case"
                title="Preserve case"
                aria-pressed={replace.preserveCase}
                disabled={!projectId}
                onClick={() =>
                  replace.onPreserveCaseChange(!replace.preserveCase)
                }
              >
                <span aria-hidden="true">AB</span>
              </button>
              <button
                type="button"
                aria-label="Replace all"
                title="Replace all"
                disabled={!projectId || searching || !replace.canReplaceAll}
                onClick={replace.onReplaceAll}
              >
                <ReplaceAll size={16} aria-hidden="true" />
              </button>
            </span>
          </div>
        </div>
      ) : null}
      {detailsOpen ? (
        <div className="workspace-search__filters">
          <label>
            <span>files to include</span>
            <input
              type="text"
              aria-label="Files to include"
              placeholder="e.g. src/**/*.ts, tests/**/*.ts"
              value={includePatternText}
              disabled={!projectId || searching}
              onChange={(event) =>
                onIncludePatternTextChange(event.target.value)
              }
            />
          </label>
          <label>
            <span>files to exclude</span>
            <input
              type="text"
              aria-label="Files to exclude"
              placeholder="e.g. **/generated/**"
              value={excludePatternText}
              disabled={!projectId || searching}
              onChange={(event) =>
                onExcludePatternTextChange(event.target.value)
              }
            />
          </label>
        </div>
      ) : null}
    </form>
  );
}
