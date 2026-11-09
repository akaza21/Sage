import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
  type RefObject,
} from "react";
import { useWorkbenchStore } from "../../app/store";
import { truncateUtf8 } from "../../domain/text";
import {
  nextQuickOpenStamp,
  type QuickOpenMatch,
} from "../quickOpen/quickOpenDomain";
import { queryQuickOpen } from "../quickOpen/quickOpenGateway";
import { recentFiles } from "../quickOpen/quickOpenState";
import { isImeComposing } from "../../ui/imeComposition";
import { SuggestionsPart } from "./composerSuggestionsPart";
import {
  suggestionOptionId,
  useSuggestionNavigation,
  type SuggestionKeyEvent,
  type TextFieldSuggestion,
  type TextFieldSuggestionsState,
} from "../../ui/textFieldSuggestionNavigation";
import {
  mentionRootQualifier,
  mentionToken,
  type MentionRoot,
} from "./composerMention";

/** Files asked of the path index per query; a short list to pick from. */
export const MENTION_RESULT_LIMIT = 20;
/** The path index takes at most this many query bytes. */
const MENTION_QUERY_BYTES = 1024;
/** While the index refreshes, the same query is asked again, boundedly. */
const REFRESH_RETRY_MS = 150;
const MAX_REFRESH_RETRIES = 40;

type Load =
  | { kind: "loading"; key: string }
  | {
      kind: "ready";
      key: string;
      results: QuickOpenMatch[];
      truncated: boolean;
    }
  | { kind: "error"; key: string; message: string };

/**
 * The project's files matching `query`, from the workspace path index that
 * Quick Open uses; no second walk. One menu opening is one index session,
 * so its first query refreshes the index in the background.
 */
function useMentionMatches(query: string): Load | null {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const epoch = useWorkbenchStore((state) => state.nativeTransitionEpoch);
  const [session] = useState(() => nextQuickOpenStamp());
  const asked = truncateUtf8(query.trim(), MENTION_QUERY_BYTES);
  const key = projectId ? JSON.stringify([projectId, epoch, asked]) : "";
  const [load, setLoad] = useState<Load | null>(null);
  useEffect(() => {
    if (!projectId) return;
    let current = true;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const ask = (attempt: number, supersededOnce: boolean) => {
      queryQuickOpen({
        projectId,
        expectedTransitionEpoch: epoch,
        query: asked,
        limit: MENTION_RESULT_LIMIT,
        generation: nextQuickOpenStamp(),
        session,
        recent: recentFiles(projectId),
      }).then(
        (response) => {
          if (!current) return;
          if (response.state !== "ready") {
            // Something newer stopped it, such as Quick Open: ask once more.
            if (!supersededOnce) ask(attempt, true);
            else
              setLoad({
                kind: "error",
                key,
                message: "The file search was interrupted. Type to retry.",
              });
            return;
          }
          setLoad({
            kind: "ready",
            key,
            results: response.results,
            truncated: response.truncated,
          });
          if (response.refreshing && attempt < MAX_REFRESH_RETRIES)
            retry = setTimeout(() => ask(attempt + 1, false), REFRESH_RETRY_MS);
        },
        (reason: unknown) => {
          if (current)
            setLoad({
              kind: "error",
              key,
              message:
                reason instanceof Error ? reason.message : String(reason),
            });
        },
      );
    };
    ask(0, false);
    return () => {
      current = false;
      clearTimeout(retry);
    };
  }, [asked, epoch, key, projectId, session]);
  if (!projectId) return null;
  // A newer query keeps the last list on screen until its answer lands.
  return load ?? { kind: "loading", key };
}

const NO_ROOTS: readonly MentionRoot[] = [];

/** The file as a pick, keyed by the token it writes; null when none can name it. */
function suggestion(
  match: QuickOpenMatch,
  roots: readonly MentionRoot[],
): TextFieldSuggestion | null {
  const qualifier = mentionRootQualifier(roots, match.rootId);
  const token = mentionToken(match.relativePath, qualifier);
  if (!token) return null;
  // A file outside the first root shows under that root's name.
  const root = qualifier && roots.find((r) => r.id === match.rootId)?.name;
  const shown = root ? `${root}/${match.relativePath}` : match.relativePath;
  const slash = shown.lastIndexOf("/");
  return {
    id: token,
    label: shown.slice(slash + 1),
    description: slash > 0 ? shown.slice(0, slash) : undefined,
    accessibleLabel: shown,
  };
}

function menuState(load: Load | null, query: string, matches: number) {
  const state: TextFieldSuggestionsState = !load
    ? { kind: "empty", title: "Open a project to mention its files." }
    : load.kind === "error"
      ? { kind: "error", title: "Files are unavailable.", detail: load.message }
      : load.kind === "loading"
        ? { kind: "loading", title: "Finding files…" }
        : matches === 0
          ? { kind: "empty", title: `No files match “${query}”.` }
          : { kind: "ready" };
  return state;
}

export function ComposerMentionMenu({
  anchorRef,
  keyHandlerRef,
  listboxId,
  onActiveOptionChange,
  onDismiss,
  onPick,
  onStatusChange,
  query,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  keyHandlerRef: RefObject<((event: SuggestionKeyEvent) => boolean) | null>;
  listboxId: string;
  onActiveOptionChange: (optionId: string | null) => void;
  onDismiss: () => void;
  /** The token that names the picked file. */
  onPick: (token: string) => void;
  /** What the list holds, for the field's polite status region. */
  onStatusChange: (status: string) => void;
  query: string;
}) {
  const load = useMentionMatches(query);
  const results = useMemo(
    () => (load?.kind === "ready" ? load.results : []),
    [load],
  );
  const roots = useWorkbenchStore((state) => state.project?.roots ?? NO_ROOTS);
  const items = useMemo(
    () =>
      results.flatMap((match) => {
        const item = suggestion(match, roots);
        return item ? [item] : [];
      }),
    [results, roots],
  );
  const pick = (item: TextFieldSuggestion) => onPick(item.id);
  const navigation = useSuggestionNavigation(items, pick, onDismiss);
  const activeId = navigation.active?.id ?? null;
  const loading = load?.kind === "loading";
  useLayoutEffect(() => {
    keyHandlerRef.current = (event) => {
      // While the first list loads, Enter must not send a half-typed path.
      if (
        loading &&
        event.key === "Enter" &&
        !event.shiftKey &&
        !isImeComposing(event)
      ) {
        event.preventDefault();
        return true;
      }
      return navigation.onKeyDown(event);
    };
  });
  useLayoutEffect(
    () => () => {
      keyHandlerRef.current = null;
    },
    [keyHandlerRef],
  );
  useLayoutEffect(() => {
    onActiveOptionChange(
      activeId ? suggestionOptionId(listboxId, activeId) : null,
    );
  }, [activeId, listboxId, onActiveOptionChange]);
  useLayoutEffect(
    () => () => onActiveOptionChange(null),
    [onActiveOptionChange],
  );
  const state = menuState(load, query, items.length);
  const status =
    state.kind === "ready"
      ? `${items.length} ${items.length === 1 ? "file" : "files"}`
      : state.title;
  useEffect(() => onStatusChange(status), [onStatusChange, status]);
  return (
    <SuggestionsPart
      part="TextFieldSuggestions"
      anchorRef={anchorRef}
      ariaLabel="Mention a file"
      className="composer-mention-menu"
      items={items}
      activeId={activeId}
      listboxId={listboxId}
      onActivate={navigation.setActiveId}
      onDismiss={onDismiss}
      onPick={pick}
      state={state}
      footer={
        load?.kind === "ready" && load.truncated
          ? "The project has more files than the index holds."
          : undefined
      }
    />
  );
}
