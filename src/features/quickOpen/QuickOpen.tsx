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

/** An answer, with the query it answers. */
type Answer = QuickOpenResponse & { query: string };

const matchKey = (match: QuickOpenMatch) =>
  `${match.rootId}:${match.relativePath}`;

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

function activeFile() {
  const state = useWorkbenchStore.getState();
  const tab = state.tabs.find((item) => item.id === state.activeTabId);
  return tab?.kind === "file" ? tab.resource : undefined;
}

export default function QuickOpen({ onClose }: { onClose: () => void }) {
  const project = useWorkbenchStore((state) => state.project);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const listId = useId();
  const [text, setText] = useState("");
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [indexing, setIndexing] = useState(false);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const session = useRef(nextQuickOpenStamp());
  const returnFocus = useRef<Element | null>(document.activeElement);
  const closed = useRef(false);
  /** Enter pressed before the answer for the current query arrived. */
  const pendingEnter = useRef(false);
  const list = useRef<HTMLUListElement | null>(null);
  const projectId = project?.id ?? null;
  const { query, line, column } = parseQuickOpenInput(text);
  // `:12` alone goes to a line of the active File; nothing is asked.
  const bareLine = query === "" && line !== null;

  const close = (restoreFocus: boolean) => {
    if (closed.current) return;
    closed.current = true;
    onClose();
    if (!restoreFocus) return;
    const previous = returnFocus.current;
    const target =
      previous instanceof HTMLElement &&
      previous !== document.body &&
      previous.isConnected
        ? previous
        : document.querySelector<HTMLElement>(".cm-content");
    target?.focus();
  };

  const open = (match: QuickOpenMatch | undefined) => {
    const state = useWorkbenchStore.getState();
    const target = bareLine ? activeFile() : match;
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
  const openLatest = useRef(open);
  useEffect(() => {
    openLatest.current = open;
  });

  useEffect(() => {
    if (!projectId || bareLine) return;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let current = true;
    const indexingTimer = setTimeout(
      () => setIndexing(true),
      INDEXING_DELAY_MS,
    );
    const fail = (message: string) => {
      clearTimeout(indexingTimer);
      setIndexing(false);
      pendingEnter.current = false;
      setError(message);
    };
    const ask = (attempt: number, supersededOnce: boolean) => {
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
          if (!current) return;
          if (response.state !== "ready") {
            // This was the newest query, so something else stopped it, such
            // as a Project closing: ask once more, then say so.
            if (!supersededOnce) ask(attempt, true);
            else fail("Quick Open was interrupted. Type to try again.");
            return;
          }
          clearTimeout(indexingTimer);
          setIndexing(false);
          setError(null);
          setAnswer({ ...response, query });
          if (pendingEnter.current) {
            pendingEnter.current = false;
            openLatest.current(response.results[0]);
            return;
          }
          if (response.refreshing && attempt < MAX_REFRESH_RETRIES) {
            retry = setTimeout(() => ask(attempt + 1, false), REFRESH_RETRY_MS);
          }
        },
        (reason: unknown) => {
          if (current) {
            fail(reason instanceof Error ? reason.message : String(reason));
          }
        },
      );
    };
    ask(0, false);
    return () => {
      current = false;
      clearTimeout(indexingTimer);
      clearTimeout(retry);
    };
  }, [bareLine, epoch, projectId, query]);

  const results = bareLine ? [] : (answer?.results ?? []);
  // The selection follows its File when a refreshed answer reorders the list.
  const keyed = results.findIndex((match) => matchKey(match) === selectedKey);
  const active = keyed >= 0 ? keyed : 0;
  const activeKey = results[active] ? matchKey(results[active]) : null;

  useEffect(() => {
    list.current
      ?.querySelector('[aria-selected="true"]')
      ?.scrollIntoView?.({ block: "nearest" });
  }, [activeKey]);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close(true);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (results.length === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = results[(active + step + results.length) % results.length];
      setSelectedKey(matchKey(next));
    } else if (event.key === "Enter") {
      // Ctrl/Cmd+Enter opens to the side where there is a split; there
      // isn't yet, so it opens in place too.
      event.preventDefault();
      if (bareLine || answer?.query === query) open(results[active]);
      // The list on screen answers an earlier query: open the first match
      // for this one once it arrives.
      else if (!error) pendingEnter.current = true;
    }
  };

  let status = "";
  if (!projectId) status = "Open a Project to find its Files.";
  else if (bareLine) {
    status = activeFile()
      ? `Press Enter to go to line ${line} in the active File.`
      : "Open a File to go to a line in it.";
  } else if (error) status = error;
  else if (!answer) status = indexing ? "Indexing…" : "";
  else if (results.length === 0) {
    status =
      query === "" ? "Type to find a File by name." : "No matching Files.";
  }

  return (
    <div
      className="quick-open-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close(true);
      }}
    >
      <div
        className="quick-open"
        role="dialog"
        aria-label="Quick Open"
        onMouseDown={(event) => {
          // Clicks inside keep the focus, and so the keys, in the input.
          if (!(event.target instanceof HTMLInputElement)) {
            event.preventDefault();
          }
        }}
      >
        <input
          className="quick-open__input"
          role="combobox"
          aria-label="Search Files by name"
          aria-autocomplete="list"
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
            setSelectedKey(null);
            pendingEnter.current = false;
          }}
          onKeyDown={onKeyDown}
          onBlur={() => close(false)}
        />
        <ul
          ref={list}
          className="quick-open__list"
          id={listId}
          role="listbox"
          aria-label="Files"
          tabIndex={-1}
        >
          {results.map((match, index) => {
            const base = match.relativePath.lastIndexOf("/") + 1;
            return (
              <li
                key={matchKey(match)}
                id={`${listId}-${index}`}
                className="quick-open__row"
                role="option"
                aria-selected={index === active}
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
        <p className="quick-open__status" role="status">
          {status}
        </p>
        {answer?.truncated && !bareLine ? (
          <p className="quick-open__status">
            Only the first {answer.indexedFiles.toLocaleString()} Files are
            searched.
          </p>
        ) : null}
      </div>
    </div>
  );
}
