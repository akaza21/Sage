import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";

import { useActiveOption } from "../../ui/listNavigation";

import { useWorkbenchStore } from "../../app/store";

import {
  highlightRuns,
  nextQuickOpenStamp,
  parseQuickOpenInput,
  QUICK_OPEN_LIMIT,
  type QuickOpenMatch,
  type QuickOpenResponse,
} from "./quickOpenDomain";
import {
  keymapBinding,
  matchesPlatformChord,
} from "../../domain/keymapRegistry";
import { usesMetaModifier } from "../../ui/shortcutLabel";
import { FileKindIcon } from "../files/FileKindIcon";
import {
  filterCommands,
  paletteCommands,
  type PaletteCommand,
} from "./paletteCommands";
import { queryQuickOpen } from "./quickOpenGateway";
import { recentFiles } from "./quickOpenState";
import { ShortcutKeys } from "./ShortcutKeys";
import "./quick-open.css";

const REFRESH_RETRY_MS = 150;
const MAX_REFRESH_RETRIES = 40;
const COMMAND_FOCUS_GRACE_MS = 150;
const INDEXING_DELAY_MS = 120;

type Answer = QuickOpenResponse & { key: string };
type Failure = { key: string; message: string };

const requestKey = (projectId: string, epoch: number, query: string) =>
  `${projectId}\u0000${epoch}\u0000${query}`;

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

function focusBack(previous: Element | null) {
  const target =
    previous instanceof HTMLElement &&
    previous !== document.body &&
    previous.isConnected
      ? previous
      : document.querySelector<HTMLElement>(".cm-content");
  target?.focus();
}

function activeFile() {
  const state = useWorkbenchStore.getState();
  const tab = state.tabs.find((item) => item.id === state.activeTabId);
  return tab?.kind === "file" ? tab.resource : undefined;
}

export default function QuickOpen({
  seed = "",
  onClose,
}: {
  seed?: string;
  onClose: () => void;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  // The command list depends on the active tab.
  useWorkbenchStore((state) => state.activeTabId);
  const listId = useId();
  const [text, setText] = useState(seed);
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [failure, setFailure] = useState<Failure | null>(null);
  const [indexing, setIndexing] = useState(false);
  const [session] = useState(() => nextQuickOpenStamp());
  const returnFocus = useRef<Element | null>(document.activeElement);
  const closed = useRef(false);
  const pendingEnter = useRef(false);
  const list = useRef<HTMLUListElement | null>(null);
  const projectId = project?.id ?? null;
  const commandMode = text.startsWith(">");
  const commands = commandMode
    ? filterCommands(paletteCommands(usesMetaModifier()), text.slice(1))
    : [];
  const { query, line, column } = parseQuickOpenInput(commandMode ? "" : text);
  const bareLine = query === "" && line !== null;
  const key = projectId ? requestKey(projectId, epoch, query) : "";
  const error = failure?.key === key ? failure.message : null;

  const close = (restoreFocus: boolean) => {
    if (closed.current) return;
    closed.current = true;
    onClose();
    if (restoreFocus) focusBack(returnFocus.current);
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
  const runCommand = (command: PaletteCommand | undefined) => {
    if (!command) return;
    if (command.run === "files") {
      setText("");
      setSelectedKey(null);
      return;
    }
    close(false);
    command.run();
    // A command that moves the focus has done so by then; one that didn't,
    // or did nothing, gets it back where it was.
    const previous = returnFocus.current;
    setTimeout(() => {
      const focused = document.activeElement;
      if (!focused || focused === document.body) focusBack(previous);
    }, COMMAND_FOCUS_GRACE_MS);
  };
  const openLatest = useRef(open);
  useEffect(() => {
    openLatest.current = open;
  });

  useEffect(() => {
    if (!projectId || bareLine || commandMode) return;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let current = true;
    const indexingTimer = setTimeout(
      () => setIndexing(true),
      INDEXING_DELAY_MS,
    );
    const asked = requestKey(projectId, epoch, query);
    const fail = (message: string) => {
      clearTimeout(indexingTimer);
      setIndexing(false);
      pendingEnter.current = false;
      setFailure({ key: asked, message });
    };
    const ask = (attempt: number, supersededOnce: boolean) => {
      queryQuickOpen({
        projectId,
        expectedTransitionEpoch: epoch,
        query,
        limit: QUICK_OPEN_LIMIT,
        generation: nextQuickOpenStamp(),
        session,
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
          setFailure(null);
          setAnswer({ ...response, key: asked });
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
  }, [bareLine, commandMode, epoch, projectId, query, session]);

  const results = bareLine || commandMode ? [] : (answer?.results ?? []);
  const itemKeys = commandMode
    ? commands.map((command) => command.id)
    : results.map(matchKey);
  const {
    activeIndex: active,
    activeId: activeKey,
    move,
    setActiveId: setSelectedKey,
  } = useActiveOption(itemKeys, "first");

  useEffect(() => {
    list.current
      ?.querySelector('[aria-selected="true"]')
      ?.scrollIntoView?.({ block: "nearest" });
    // A refreshed answer can move the selected File without changing it.
  }, [activeKey, active]);

  const confirmFile = (match: QuickOpenMatch | undefined) => {
    if (bareLine || answer?.key === key) open(match);
    else if (!error) pendingEnter.current = true;
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    // keyCode 229 is the key that ends a composition, which WebKit (macOS)
    // reports after `compositionend`.
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) {
      return;
    }
    const apple = usesMetaModifier();
    const files = matchesPlatformChord(
      keymapBinding("quickOpen"),
      event,
      apple,
    );
    if (
      files ||
      matchesPlatformChord(keymapBinding("commandPalette"), event, apple)
    ) {
      event.preventDefault();
      event.stopPropagation();
      setText(files ? "" : ">");
      setSelectedKey(null);
      pendingEnter.current = false;
    } else if (event.key === "Escape") {
      event.preventDefault();
      close(true);
    } else if (
      event.key === "ArrowDown" ||
      event.key === "ArrowUp" ||
      event.key === "PageDown" ||
      event.key === "PageUp"
    ) {
      event.preventDefault();
      move(event.key);
    } else if (event.key === "Enter") {
      // Ctrl/Cmd+Enter opens to the side where there is a split; there
      // isn't yet, so it opens in place too.
      event.preventDefault();
      if (commandMode) runCommand(commands[active]);
      else confirmFile(results[active]);
    }
  };

  let status = "";
  if (commandMode) {
    status = commands.length === 0 ? "No matching commands." : "";
  } else if (!projectId) status = "Open a project to find its files.";
  else if (bareLine) {
    status = activeFile()
      ? `Press Enter to go to line ${line} in the active file.`
      : "Open a file to go to a line in it.";
  } else if (error) status = error;
  else if (!answer) status = indexing ? "Indexing…" : "";
  else if (results.length === 0) {
    status =
      query === "" ? "Type to find a file by name." : "No matching files.";
  }

  return (
    <div
      className="quick-open-backdrop"
      onMouseDown={(event) => {
        if (event.target !== event.currentTarget) return;
        // Otherwise the click would move the focus back off the editor.
        event.preventDefault();
        close(true);
      }}
    >
      <div
        className="quick-open popup-material"
        role="dialog"
        aria-modal="true"
        aria-label="Quick Open"
        onMouseDown={(event) => {
          if (!(event.target instanceof HTMLInputElement)) {
            event.preventDefault();
          }
        }}
      >
        {/* Chromium drops a focused input's placeholder ellipsis, and this
            input always has focus, so the hint is drawn over it instead. */}
        <input
          className="quick-open__input"
          role="combobox"
          aria-label={commandMode ? "Run a command" : "Search files by name"}
          aria-autocomplete="list"
          aria-expanded={itemKeys.length > 0}
          aria-controls={listId}
          aria-activedescendant={
            itemKeys.length > 0 ? `${listId}-${active}` : undefined
          }
          autoFocus
          spellCheck={false}
          autoComplete="off"
          aria-describedby={`${listId}-hint`}
          value={text}
          onChange={(event) => {
            const next = event.target.value;
            setText(next);
            if (parseQuickOpenInput(next).query !== query) setSelectedKey(null);
            pendingEnter.current = false;
          }}
          onKeyDown={onKeyDown}
          onBlur={() => close(false)}
        />
        <span
          id={`${listId}-hint`}
          className="quick-open__hint"
          aria-hidden="true"
          hidden={text !== ""}
        >
          Search files by name (:line goes to a line, &gt; runs a command)
        </span>
        <ul
          ref={list}
          className="quick-open__list"
          id={listId}
          role="listbox"
          aria-label={commandMode ? "Commands" : "Files"}
          tabIndex={-1}
        >
          {commands.map((command, index) => (
            <li
              key={command.id}
              id={`${listId}-${index}`}
              className="quick-open__row"
              role="option"
              aria-selected={index === active}
              onClick={() => runCommand(command)}
            >
              <span className="quick-open__name">{command.label}</span>
              {command.shortcut ? (
                <ShortcutKeys label={command.shortcut} />
              ) : null}
            </li>
          ))}
          {results.map((match, index) => {
            const base = match.relativePath.lastIndexOf("/") + 1;
            return (
              <li
                key={matchKey(match)}
                id={`${listId}-${index}`}
                className="quick-open__row"
                role="option"
                aria-selected={index === active}
                onClick={() => confirmFile(match)}
              >
                <span className="quick-open__icon">
                  <FileKindIcon path={match.relativePath} size={16} />
                </span>
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
        {answer?.truncated && !bareLine && !commandMode ? (
          <p className="quick-open__status">
            Only the first {answer.indexedFiles.toLocaleString()} files are
            searched.
          </p>
        ) : null}
      </div>
    </div>
  );
}
