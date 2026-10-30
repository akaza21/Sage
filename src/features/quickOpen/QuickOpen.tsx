import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";

import { useWorkbenchStore } from "../../app/store";

import {
  highlightRuns,
  nextQuickOpenStamp,
  parseQuickOpenInput,
  QUICK_OPEN_LIMIT,
  type QuickOpenMatch,
  type QuickOpenResponse,
} from "./quickOpenDomain";
import { queryQuickOpen } from "./quickOpenGateway";
import { recentFiles } from "./quickOpenState";
import "./quick-open.css";

/** While the index is being refreshed, the same query is asked again. */
const REFRESH_RETRY_MS = 150;
const MAX_REFRESH_RETRIES = 40;
/** "Indexing…" shows only if the first answer takes longer than this. */
const INDEXING_DELAY_MS = 120;

function Highlighted({
  text,
  offset,
  positions,
}: {
  text: string;
  offset: number;
  positions: readonly number[];
}) {
  return highlightRuns(text, offset, positions).map((run, index) =>
    run.matched ? (
      <span key={index} className="quick-open__match">
        {run.text}
      </span>
    ) : (
      run.text
    ),
  );
}

export default function QuickOpen({ onClose }: { onClose: () => void }) {
  const project = useWorkbenchStore((state) => state.project);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const listId = useId();
  const [text, setText] = useState("");
  const [answer, setAnswer] = useState<QuickOpenResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [indexing, setIndexing] = useState(false);
  const [selected, setSelected] = useState(0);
  const session = useRef(nextQuickOpenStamp());
  const returnFocus = useRef<Element | null>(document.activeElement);
  const projectId = project?.id ?? null;

  useEffect(() => {
    if (!projectId) return;
    const { query } = parseQuickOpenInput(text);
    let retry: ReturnType<typeof setTimeout> | undefined;
    let current = true;
    const indexingTimer = setTimeout(
      () => setIndexing(true),
      INDEXING_DELAY_MS,
    );
    const ask = (attempt: number) => {
      queryQuickOpen({
        projectId,
        expectedTransitionEpoch: epoch,
        query,
        limit: QUICK_OPEN_LIMIT,
        generation: nextQuickOpenStamp(),
        session: session.current,
        recent: recentFiles(projectId),
      }).then(
        (response) => {
          if (!current || response.state !== "ready") return;
          clearTimeout(indexingTimer);
          setIndexing(false);
          setError(null);
          setAnswer(response);
          if (response.refreshing && attempt < MAX_REFRESH_RETRIES) {
            retry = setTimeout(() => ask(attempt + 1), REFRESH_RETRY_MS);
          }
        },
        (reason: unknown) => {
          if (!current) return;
          clearTimeout(indexingTimer);
          setIndexing(false);
          setError(reason instanceof Error ? reason.message : String(reason));
        },
      );
    };
    ask(0);
    return () => {
      current = false;
      clearTimeout(indexingTimer);
      clearTimeout(retry);
    };
  }, [epoch, projectId, text]);

  const closed = useRef(false);
  const close = (restoreFocus: boolean) => {
    if (closed.current) return;
    closed.current = true;
    onClose();
    if (restoreFocus && returnFocus.current instanceof HTMLElement) {
      returnFocus.current.focus();
    }
  };

  const { query, line, column } = parseQuickOpenInput(text);
  const results = answer?.results ?? [];
  const active = Math.min(selected, results.length - 1);

  const open = (match: QuickOpenMatch | undefined) => {
    const state = useWorkbenchStore.getState();
    const target =
      match ??
      // `:12` alone goes to a line of the active File.
      (query === "" && line
        ? state.tabs.find(
            (tab) => tab.id === state.activeTabId && tab.kind === "file",
          )?.resource
        : undefined);
    if (!projectId || !target) return;
    const resource = {
      rootId: target.rootId,
      relativePath: target.relativePath,
    };
    close(false);
    if (line) {
      void state.openFileLocation({
        projectId,
        transitionEpoch: epoch,
        resource,
        lineNumber: line,
        columnNumber: column ?? 1,
        selection: { kind: "caret" },
      });
    } else {
      void state.openFile(resource);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close(true);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (results.length === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelected((active + step + results.length) % results.length);
    } else if (event.key === "Enter") {
      // Ctrl/Cmd+Enter opens to the side where there is a split; there
      // isn't yet, so it opens in place too.
      event.preventDefault();
      open(results[active]);
    }
  };

  let status: string | null = null;
  if (!projectId) status = "Open a Project to find its Files.";
  else if (error) status = error;
  else if (!answer) status = indexing ? "Indexing…" : null;
  else if (results.length === 0) {
    status =
      query === "" && line
        ? `Press Enter to go to line ${line} in the active File.`
        : query === ""
          ? "Type to find a File by name."
          : "No matching Files.";
  }

  return (
    <div
      className="quick-open-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close(true);
      }}
    >
      <div className="quick-open" role="dialog" aria-label="Quick Open">
        <input
          className="quick-open__input"
          role="combobox"
          aria-label="Search Files by name"
          aria-expanded={results.length > 0}
          aria-controls={listId}
          aria-activedescendant={
            results.length > 0 ? `${listId}-${active}` : undefined
          }
          autoFocus
          spellCheck={false}
          autoComplete="off"
          placeholder="Search Files by name (append :line to go to a line)"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            setSelected(0);
          }}
          onKeyDown={onKeyDown}
          onBlur={(event) => {
            if (
              !event.currentTarget
                .closest(".quick-open")
                ?.contains(event.relatedTarget)
            ) {
              close(false);
            }
          }}
        />
        <ul
          className="quick-open__list"
          id={listId}
          role="listbox"
          aria-label="Files"
        >
          {results.map((match, index) => {
            const base = match.relativePath.lastIndexOf("/") + 1;
            return (
              <li
                key={`${match.rootId}:${match.relativePath}`}
                id={`${listId}-${index}`}
                className="quick-open__row"
                role="option"
                aria-selected={index === active}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => open(match)}
              >
                <span className="quick-open__name">
                  <Highlighted
                    text={match.relativePath.slice(base)}
                    offset={base}
                    positions={match.positions}
                  />
                </span>
                {base > 0 ? (
                  <span className="quick-open__folder">
                    <Highlighted
                      text={match.relativePath.slice(0, base - 1)}
                      offset={0}
                      positions={match.positions}
                    />
                  </span>
                ) : null}
              </li>
            );
          })}
        </ul>
        {status ? (
          <p className="quick-open__status" role="status">
            {status}
          </p>
        ) : null}
        {answer?.truncated ? (
          <p className="quick-open__status">
            Only the first {answer.indexedFiles.toLocaleString()} Files are
            searched.
          </p>
        ) : null}
      </div>
    </div>
  );
}
