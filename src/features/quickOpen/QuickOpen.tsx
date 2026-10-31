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
import {
  keymapBinding,
  matchesPlatformChord,
} from "../../domain/keymapRegistry";
import { usesMetaModifier } from "../../ui/shortcutLabel";
import {
  filterCommands,
  paletteCommands,
  type PaletteCommand,
} from "./paletteCommands";
import { queryQuickOpen } from "./quickOpenGateway";
import { recentFiles } from "./quickOpenState";
import "./quick-open.css";

/** While the index is being refreshed, the same query is asked again. */
const REFRESH_RETRY_MS = 150;
const MAX_REFRESH_RETRIES = 40;
/** How long a command has to move the focus before the box puts it back. */
const COMMAND_FOCUS_GRACE_MS = 150;
/** "Indexing…" shows only if the first answer takes longer than this. */
const INDEXING_DELAY_MS = 120;

/** An answer or a failure, with the request it answers. */
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

/** Back to `previous`, or to the editor when that is gone. */
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
  /** What the box starts with: ">" lists commands. */
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
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [session] = useState(() => nextQuickOpenStamp());
  const returnFocus = useRef<Element | null>(document.activeElement);
  const closed = useRef(false);
  /** Enter pressed before the answer for the current query arrived. */
  const pendingEnter = useRef(false);
  const list = useRef<HTMLUListElement | null>(null);
  const projectId = project?.id ?? null;
  // ">" lists the editor's commands instead of Files (ED-2a).
  const commandMode = text.startsWith(">");
  const commands = commandMode
    ? filterCommands(paletteCommands(usesMetaModifier()), text.slice(1))
    : [];
  const { query, line, column } = parseQuickOpenInput(commandMode ? "" : text);
  // `:12` alone goes to a line of the active File; nothing is asked.
  const bareLine = query === "" && line !== null;
  const key = projectId ? requestKey(projectId, epoch, query) : "";
  // A failure counts only for the request it answered.
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
  // The selection follows its File when a refreshed answer reorders the list.
  const keyed = itemKeys.indexOf(selectedKey ?? "");
  const active = keyed >= 0 ? keyed : 0;
  const activeKey = itemKeys[active] ?? null;

  useEffect(() => {
    list.current
      ?.querySelector('[aria-selected="true"]')
      ?.scrollIntoView?.({ block: "nearest" });
    // A refreshed answer can move the selected File without changing it.
  }, [activeKey, active]);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    // keyCode 229 is the key that ends a composition, which WebKit (macOS)
    // reports after `compositionend`.
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) {
      return;
    }
    // The chords that open the box switch it between Files and commands.
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
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (itemKeys.length === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelectedKey(
        itemKeys[(active + step + itemKeys.length) % itemKeys.length],
      );
    } else if (event.key === "Enter") {
      // Ctrl/Cmd+Enter opens to the side where there is a split; there
      // isn't yet, so it opens in place too.
      event.preventDefault();
      if (commandMode) runCommand(commands[active]);
      else if (bareLine || answer?.key === key) open(results[active]);
      // The list on screen answers an earlier request: open the first match
      // for this one once it arrives.
      else if (!error) pendingEnter.current = true;
    }
  };

  let status = "";
  if (commandMode) {
    status = commands.length === 0 ? "No matching commands." : "";
  } else if (!projectId) status = "Open a Project to find its Files.";
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
        if (event.target !== event.currentTarget) return;
        // Otherwise the click would move the focus back off the editor.
        event.preventDefault();
        close(true);
      }}
    >
      <div
        className="quick-open"
        role="dialog"
        aria-modal="true"
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
          aria-label={commandMode ? "Run a command" : "Search Files by name"}
          aria-autocomplete="list"
          aria-expanded={itemKeys.length > 0}
          aria-controls={listId}
          aria-activedescendant={
            itemKeys.length > 0 ? `${listId}-${active}` : undefined
          }
          autoFocus
          spellCheck={false}
          autoComplete="off"
          placeholder="Search Files by name (:line goes to a line, > runs a command)"
          value={text}
          onChange={(event) => {
            const next = event.target.value;
            setText(next);
            // Typing a :line suffix keeps the chosen File.
            if (parseQuickOpenInput(next).query !== query) setSelectedKey(null);
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
                <kbd className="quick-open__shortcut">{command.shortcut}</kbd>
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
        {answer?.truncated && !bareLine && !commandMode ? (
          <p className="quick-open__status">
            Only the first {answer.indexedFiles.toLocaleString()} Files are
            searched.
          </p>
        ) : null}
      </div>
    </div>
  );
}
