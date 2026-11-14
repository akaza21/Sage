import { isolateHistory } from "@codemirror/commands";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentType,
  type KeyboardEvent,
  type RefObject,
} from "react";

import {
  CaseSensitive,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Regex,
  Replace,
  ReplaceAll,
  WholeWord,
  X,
} from "../../ui/leanIcons";
import { usesMetaModifier } from "../../ui/shortcutLabel";
import type { EditorAdapterHandle } from "./EditorAdapter";
import { clearMatches, showMatches } from "./findMarks";
import {
  MAX_EDITABLE_CONTENT_BYTES,
  MAX_FIND_QUERY_LENGTH,
  contentText,
  type EditorContent,
} from "./editorContent";
import {
  createFindWorkerClient,
  findMatches,
  type FindMatches,
  type FindOptions,
} from "./inFileFind";

export interface FindOpenRequest {
  replace: boolean;
  seed: string | null;
}

export interface FindControl {
  move: (direction: 1 | -1) => void;
}

interface FileFindBarProps {
  request: FindOpenRequest;
  inputRef: RefObject<HTMLInputElement | null>;
  controlRef: RefObject<FindControl | null>;
  editorRef: RefObject<EditorAdapterHandle | null>;
  content: EditorContent;
  canReplace: boolean;
  onClose: () => void;
}

const remembered = {
  query: "",
  replacement: "",
  options: { caseSensitive: false, wholeWord: false, regex: false },
};

const ERRORS = {
  invalid: "Invalid pattern",
  inlineFlag: "Use Match case, not an inline flag",
  empty: "Pattern matches empty text",
  timeout: "Pattern stopped after 250 ms",
  unavailable: "Pattern search is unavailable",
};

const TOGGLES = [
  ["Match case", CaseSensitive, "caseSensitive", "KeyC"],
  ["Match whole word", WholeWord, "wholeWord", "KeyW"],
  ["Use regular expression", Regex, "regex", "KeyR"],
] as const;

const ORIGIN = { start: 0, end: 0 };
// Past the ceiling a File is refused before any scan.
const large = (text: string) =>
  text.length > 64 * 1024 && text.length <= MAX_EDITABLE_CONTENT_BYTES;
type Found = { key: readonly unknown[]; matches: FindMatches };

const spawnWorker = () =>
  new Worker(new URL("./inFileFindWorker.ts", import.meta.url), {
    type: "module",
  });

export function FileFindBar({
  request,
  inputRef,
  controlRef,
  editorRef,
  content,
  canReplace,
  onClose,
}: FileFindBarProps) {
  const [query, setQuery] = useState(request.seed ?? remembered.query);
  const [replacement, setReplacement] = useState(remembered.replacement);
  const [options, setOptions] = useState<FindOptions>(remembered.options);
  const [replaceOpen, setReplaceOpen] = useState(request.replace);
  const [shownRequest, setShownRequest] = useState(request);
  const [anchor, setAnchor] = useState(
    () => editorRef.current?.selection().start ?? 0,
  );
  // Counts moves, so a move back to the same match still selects it.
  const [moves, setMoves] = useState(0);
  const [found, setFound] = useState<Found | null>(null);
  const replaceRef = useRef<HTMLInputElement>(null);
  const client = useMemo(() => createFindWorkerClient(spawnWorker), []);
  // Its own worker, so a Replace never displaces a waiting search.
  const replacer = useMemo(() => createFindWorkerClient(spawnWorker), []);
  if (request !== shownRequest) {
    setShownRequest(request);
    if (request.seed !== null) setQuery(request.seed);
    if (request.replace) setReplaceOpen(true);
  }

  const [searched, setSearched] = useState({ content, query, options });
  if (searched.query !== query || searched.options !== options)
    setSearched({ content, query, options });
  const fresh = searched.content === content;
  const replaced = useRef(false);
  useEffect(() => {
    if (fresh) return;
    const refresh = () => setSearched((s) => ({ ...s, content }));
    if (replaced.current) {
      replaced.current = false;
      return refresh();
    }
    const timer = setTimeout(refresh, 100);
    return () => clearTimeout(timer);
  }, [content, fresh]);
  const text = useMemo(() => contentText(searched.content), [searched.content]);
  const inWorker = query !== "" && (options.regex || large(text));
  const literal = useMemo(
    () => (inWorker ? null : findMatches({ content: text, query, options })),
    [inWorker, options, query, text],
  );
  const last =
    found && found.key[1] === query && found.key[2] === options ? found : null;
  const matches = literal ?? (last?.key[0] === text ? last.matches : null);
  const shown = matches ?? last?.matches ?? null;
  const ranges = useMemo(() => shown?.ranges ?? [], [shown]);
  const count = ranges.length / 2;
  let index = count > 0 ? 0 : -1;
  while (index >= 0 && index < count && ranges[index * 2]! < anchor) index++;
  if (index === count) index = 0;

  useEffect(() => {
    Object.assign(remembered, { query, replacement, options });
  });
  useEffect(() => {
    if (!inWorker) return;
    let live = true;
    const request = { content: text, query, options };
    void client.run(request).then((result) => {
      if (live && result)
        setFound({ key: [text, query, options], matches: result });
    });
    return () => void (live = false);
  }, [client, inWorker, options, query, text]);
  useEffect(() => {
    const box = replaceRef.current;
    const target =
      shownRequest.replace && remembered.query && box && !box.disabled
        ? box
        : inputRef.current;
    target?.focus();
    target?.select();
  }, [inputRef, shownRequest]);

  // Keyed on the search, not the content, so typing in the editor with the bar
  // open keeps the caret.
  const selectKey = `${anchor}\0${moves}\0${query}\0${Object.values(options).join()}`;
  const selectedRef = useRef({ key: "", start: -1, end: -1 });
  useEffect(() => {
    const editor = editorRef.current;
    if (!editor || !matches || !fresh) return;
    showMatches(editor.view(), ranges, index);
    if (selectedRef.current.key === selectKey) return;
    const previous = selectedRef.current;
    const { start, end } = editor.selection();
    const at = index >= 0 ? ranges.slice(index * 2, index * 2 + 2) : [-1, -1];
    selectedRef.current = { key: selectKey, start: at[0]!, end: at[1]! };
    if (index >= 0) editor.setSelection(at[0]!, at[1]!);
    else if (start === previous.start && end === previous.end)
      editor.setSelection(start, start);
  }, [editorRef, fresh, index, matches, ranges, selectKey]);
  useEffect(
    () => () => {
      client.dispose();
      replacer.dispose();
      showMatches(editorRef.current?.view(), [], -1);
    },
    [client, editorRef, replacer],
  );

  const pendingMove = useRef<1 | -1 | null>(null);
  const move = (direction: 1 | -1) => {
    pendingMove.current = matches && fresh ? null : direction;
    if (!fresh) setSearched((s) => ({ ...s, content }));
    if (pendingMove.current || count === 0) return;
    const { start, end } = editorRef.current?.selection() ?? ORIGIN;
    const starts = Array.from({ length: count }, (_, at) => ranges[at * 2]!);
    const after = starts.findIndex((from) => from >= end);
    const before = starts.filter((from) => from < start).length - 1;
    const next =
      direction === 1 ? Math.max(after, 0) : before < 0 ? count - 1 : before;
    setAnchor(ranges[next * 2]!);
    setMoves((value) => value + 1);
  };
  useEffect(() => {
    controlRef.current = { move };
    // A move asked for while a pattern search ran applies to its result.
    if (matches && fresh && pendingMove.current) move(pendingMove.current);
  });

  async function replace(all: boolean) {
    const view = editorRef.current?.view();
    if (!canReplace || !view || view.state.readOnly) return;
    const doc = view.state.doc;
    const content = doc.toString();
    const request = { content, query, options, replacement, limit: Infinity };
    const result =
      options.regex || large(content)
        ? await replacer.run(request)
        : findMatches(request);
    if (!result || view.state.doc !== doc) return;
    if (result.error)
      return setFound({ key: [text, query, options], matches: result });
    const hits = result.ranges;
    const { from, to } = view.state.selection.main;
    let first = 0;
    let end = hits.length / 2;
    if (!all) {
      while (first < end && hits[first * 2] !== from) first++;
      if (first === end || hits[first * 2 + 1] !== to) return move(1);
      end = first + 1;
    }
    const changes = [];
    for (let match = first; match < end; match += 1) {
      changes.push({
        from: hits[match * 2]!,
        to: hits[match * 2 + 1]!,
        insert: result.inserts?.[match] ?? replacement,
      });
    }
    if (changes.length === 0) return;
    // One transaction, so Replace all is a single undo step.
    view.dispatch({
      changes,
      effects: all ? clearMatches() : [],
      annotations: isolateHistory.of("full"),
      userEvent: all ? "input.replace.all" : "input.replace",
    });
    replaced.current = true;
    if (!all) setAnchor(changes[0]!.from + changes[0]!.insert.length);
  }

  const toggle = (option: keyof FindOptions) =>
    setOptions((value) => ({ ...value, [option]: !value[option] }));
  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const option = TOGGLES.find((entry) => entry[3] === event.code)?.[2];
    const { altKey, metaKey, ctrlKey, shiftKey } = event;
    if (!option || !altKey || ctrlKey || shiftKey) return;
    if (metaKey !== usesMetaModifier()) return;
    event.preventDefault();
    toggle(option);
  }

  const truncated = shown?.truncated === true;
  const status =
    query.length === 0
      ? "No query"
      : !shown
        ? "Searching…"
        : shown.error
          ? ERRORS[shown.error]
          : shown.contentLimitExceeded
            ? "File exceeds the 1 MiB Find limit"
            : count === 0
              ? "No matches"
              : `${index + 1} of ${count}${truncated ? "+" : ""} ${count === 1 && !truncated ? "match" : "matches"}`;
  const button = (
    label: string,
    Icon: ComponentType<{ size: number; "aria-hidden": "true" }>,
    onClick: () => void,
    extra: { disabled?: boolean; pressed?: boolean } = {},
  ) => (
    <button
      key={label}
      className="icon-button editor-find__button"
      type="button"
      aria-label={label}
      aria-pressed={extra.pressed}
      disabled={extra.disabled}
      onClick={onClick}
    >
      <Icon size={14} aria-hidden="true" />
    </button>
  );

  return (
    <div
      className={`editor-find editor-find--overlay${replaceOpen ? " editor-find--replacing" : ""}`}
      role="search"
      aria-label="Find in file"
      onKeyDown={onKeyDown}
    >
      <button
        className="icon-button editor-find__button editor-find__expand"
        type="button"
        aria-label="Toggle Replace"
        aria-expanded={replaceOpen}
        onClick={() => setReplaceOpen((open) => !open)}
      >
        {replaceOpen ? (
          <ChevronDown size={14} aria-hidden="true" />
        ) : (
          <ChevronRight size={14} aria-hidden="true" />
        )}
      </button>
      <div className="editor-find__field">
        <label className="editor-find__query">
          <span className="sr-only">Find text</span>
          <input
            ref={inputRef}
            type="text"
            aria-label="Find text"
            maxLength={MAX_FIND_QUERY_LENGTH}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                move(event.shiftKey ? -1 : 1);
              }
            }}
          />
        </label>
        {TOGGLES.map(([label, Icon, option]) =>
          button(label, Icon, () => toggle(option), {
            pressed: options[option],
          }),
        )}
      </div>
      <span className="editor-find__status" role="status">
        {status}
      </span>
      {button("Previous match", ChevronUp, () => move(-1), {
        disabled: count === 0,
      })}
      {button("Next match", ChevronDown, () => move(1), {
        disabled: count === 0,
      })}
      {button("Close find", X, onClose)}
      {replaceOpen ? (
        <div className="editor-find__replace">
          <div className="editor-find__field">
            <label className="editor-find__query">
              <span className="sr-only">Replace text</span>
              <input
                ref={replaceRef}
                type="text"
                aria-label="Replace text"
                disabled={!canReplace}
                value={replacement}
                onChange={(event) => setReplacement(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "Enter") return;
                  event.preventDefault();
                  void replace(
                    !event.shiftKey &&
                      (usesMetaModifier()
                        ? event.metaKey && !event.altKey
                        : event.ctrlKey && event.altKey),
                  );
                }}
              />
            </label>
          </div>
          <div className="editor-find__actions">
            {button("Replace", Replace, () => void replace(false), {
              disabled: !canReplace || count === 0,
            })}
            {button("Replace all", ReplaceAll, () => void replace(true), {
              disabled: !canReplace || count === 0,
            })}
          </div>
        </div>
      ) : null}
    </div>
  );
}
