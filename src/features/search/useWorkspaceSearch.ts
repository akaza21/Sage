import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { utf8ByteLength } from "../../domain/text";
import { subscribeWorkspaceNotifySearch } from "../files/workspaceNotifyCompose";
import { sameSearchSettings, searchSettingFields } from "./searchSettings";
import type {
  WorkspaceSearchGateway,
  WorkspaceSearchResponse,
} from "./workspaceSearchDomain";
import { loadWorkspaceSearchChecks } from "./workspaceSearchDesktop";
import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";

export type WorkspaceSearchPhase = "idle" | "searching" | "results" | "error";

export interface WorkspaceSearchState {
  phase: WorkspaceSearchPhase;
  query: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  useRegex: boolean;
  includePatternText: string;
  excludePatternText: string;
  response: WorkspaceSearchResponse | null;
  pages: WorkspaceSearchResponse[];
  error: string | null;
}

interface UseWorkspaceSearchOptions {
  projectId: string | null;
  transitionEpoch: number;
  indexGeneration: number;
  gateway: WorkspaceSearchGateway;
}

const initialState = (): WorkspaceSearchState => ({
  phase: "idle",
  query: "",
  caseSensitive: false,
  wholeWord: false,
  useRegex: false,
  includePatternText: "",
  excludePatternText: "",
  response: null,
  pages: [],
  error: null,
});

/** A brace that opens no whole `{…}` group is an ordinary character. */
function splitFilterField(value: string): string[] {
  const pieces = value.match(/(?:[^,{]|\{[^{}]*\}|\{)+/g) ?? [];
  return pieces.map((piece) => piece.trim()).filter(Boolean);
}

/** Returns `null` on overflow so the caller refuses the whole field instead of sending part of it. */
function expandBraces(
  piece: string,
  limit: number,
  maxBytes: number,
): string[] | null {
  const groups = /\{([^{}]*)\}/g;
  let results = [""];
  let end = 0;
  for (let group = groups.exec(piece); group; group = groups.exec(piece)) {
    const head = piece.slice(end, group.index);
    const next = new Set<string>();
    for (const prefix of results) {
      for (const choice of group[1].split(",")) {
        // UTF-16 length never exceeds the UTF-8 byte length.
        const text = prefix + head + choice;
        if (text.length > maxBytes || next.add(text).size > limit) return null;
      }
    }
    results = [...next];
    end = groups.lastIndex;
  }
  return results.map((prefix) => prefix + piece.slice(end));
}

/**
 * Reads one include or exclude piece as VS Code does, in Sage's whole-path
 * dialect: a name with no `/` matches at any depth (and, without wildcards,
 * everything below it too), a trailing `/` means the folder's contents and a
 * leading `./` anchors to the root. A piece that names a path is unchanged.
 */
function expandFilterPiece(piece: string): string[] {
  const pattern = piece.replace(/^(?:\.\/)+/, "");
  const anchored = pattern !== piece;
  const folder = pattern.replace(/\/+$/, "");
  if (folder !== pattern) return folder ? [`${folder}/**`] : [];
  if (!pattern) return [];
  if (pattern.includes("/") || pattern === "**") return [pattern];
  const prefixed = anchored ? pattern : `**/${pattern}`;
  return /[*?]/.test(pattern) ? [prefixed] : [prefixed, `${prefixed}/**`];
}

/** Null when the patterns exceed the limits. */
function filterPatterns(value: string): string[] | null {
  const { filterPatterns: limit, filterBytes } = WORKSPACE_SEARCH_LIMITS;
  const patterns = new Set<string>();
  for (const piece of splitFilterField(value)) {
    const choices = expandBraces(piece, limit, filterBytes);
    if (!choices) return null;
    for (const choice of choices) {
      for (const pattern of expandFilterPiece(choice.trim())) {
        if (patterns.add(pattern).size > limit) return null;
      }
    }
  }
  return [...patterns];
}

function searchFilters(includeText: string, excludeText: string) {
  const includePatterns = filterPatterns(includeText);
  const excludePatterns = filterPatterns(excludeText);
  if (
    !includePatterns ||
    !excludePatterns ||
    utf8ByteLength([...includePatterns, ...excludePatterns].join("")) >
      WORKSPACE_SEARCH_LIMITS.filterBytes
  ) {
    throw new Error("workspace Search path filters are invalid");
  }
  return { includePatterns, excludePatterns };
}

type CriterionKey = Exclude<
  keyof WorkspaceSearchState,
  "phase" | "response" | "pages" | "error"
>;

const clearedResults = {
  phase: "idle",
  response: null,
  pages: [],
  error: null,
} satisfies Partial<WorkspaceSearchState>;

export function useWorkspaceSearch({
  projectId,
  transitionEpoch,
  indexGeneration,
  gateway,
}: UseWorkspaceSearchOptions) {
  const [state, setState] = useState<WorkspaceSearchState>(initialState);
  const useIgnoreFiles = useEditorSettingsStore(
    (store) => store.settings["search.useIgnoreFiles"],
  );
  const filesExclude = useEditorSettingsStore(
    (store) => store.settings["files.exclude"],
  );
  const active = useRef<AbortController | null>(null);
  const generation = useRef(0);

  // Retire the previous context before the input can accept events. A passive
  // reset can erase typing on a freshly revealed lazy surface.
  useLayoutEffect(() => {
    generation.current += 1;
    active.current?.abort();
    active.current = null;
    setState(initialState());
    return () => active.current?.abort();
  }, [indexGeneration, projectId, transitionEpoch]);

  const retireCriteria = useCallback(() => {
    generation.current += 1;
    active.current?.abort();
    active.current = null;
  }, []);

  useEffect(
    () =>
      useEditorSettingsStore.subscribe((store, previous) => {
        if (sameSearchSettings(store.settings, previous.settings)) return;
        retireCriteria();
        setState((current) => ({ ...current, ...clearedResults }));
      }),
    [retireCriteria],
  );

  const setCriterion = useCallback(
    <K extends CriterionKey>(key: K, value: WorkspaceSearchState[K]) => {
      retireCriteria();
      setState((current) => ({ ...current, ...clearedResults, [key]: value }));
    },
    [retireCriteria],
  );
  const setters = useMemo(
    () => ({
      setQuery: (query: string) => setCriterion("query", query),
      setCaseSensitive: (on: boolean) => setCriterion("caseSensitive", on),
      setWholeWord: (on: boolean) => setCriterion("wholeWord", on),
      setUseRegex: (on: boolean) => setCriterion("useRegex", on),
      setIncludePatternText: (text: string) =>
        setCriterion("includePatternText", text),
      setExcludePatternText: (text: string) =>
        setCriterion("excludePatternText", text),
    }),
    [setCriterion],
  );

  const cancel = useCallback(() => {
    generation.current += 1;
    active.current?.abort();
    active.current = null;
    setState((current) => ({ ...current, ...clearedResults }));
  }, []);

  const runSearch = useCallback(
    async (more: boolean) => {
      const query = state.query.trim();
      const token = more ? state.pages.at(-1)?.continuationToken : null;
      if (more) {
        if (
          !projectId ||
          !query ||
          !token ||
          state.pages.length >= WORKSPACE_SEARCH_LIMITS.continuationPages
        ) {
          return;
        }
      } else if (!projectId || !query) {
        setState((current) => ({
          ...current,
          phase: "error",
          response: null,
          error: projectId
            ? "Enter text to search for."
            : "Open a project before searching.",
        }));
        return;
      }

      active.current?.abort();
      const controller = new AbortController();
      active.current = controller;
      const requestGeneration = generation.current + 1;
      generation.current = requestGeneration;
      const fresh = more ? {} : { query, response: null };
      setState((current) => ({
        ...current,
        ...fresh,
        phase: "searching",
        error: null,
      }));

      const checksLoad = loadWorkspaceSearchChecks();
      // A failed load surfaces where it is awaited, not as an unhandled one.
      void checksLoad.catch(() => undefined);
      try {
        const response = await gateway.search(
          {
            projectId,
            expectedTransitionEpoch: transitionEpoch,
            indexGeneration,
            continuationToken: token ?? null,
            query,
            caseSensitive: state.caseSensitive,
            wholeWord: state.wholeWord,
            ...(state.useRegex ? { useRegex: true } : {}),
            ...searchFilters(
              state.includePatternText,
              state.excludePatternText,
            ),
            ...searchSettingFields(useIgnoreFiles, filesExclude),
          },
          controller.signal,
        );
        const checks = await checksLoad;
        if (
          controller.signal.aborted ||
          generation.current !== requestGeneration
        ) {
          return;
        }
        const validated = checks.parseWorkspaceSearchResponse(response);
        if (
          validated.projectId !== projectId ||
          validated.transitionEpoch !== transitionEpoch ||
          validated.indexGeneration !== indexGeneration ||
          validated.query !== query
        ) {
          throw new Error("invalid workspace Search response");
        }
        active.current = null;
        setState((current) => ({
          ...current,
          phase: "results",
          response: validated,
          pages: more
            ? [...current.pages, validated].slice(
                0,
                WORKSPACE_SEARCH_LIMITS.continuationPages,
              )
            : [validated],
          error: null,
        }));
      } catch (error: unknown) {
        if (
          controller.signal.aborted ||
          generation.current !== requestGeneration
        ) {
          return;
        }
        active.current = null;
        setState((current) => ({
          ...current,
          ...fresh,
          phase: "error",
          error: errorMessage(error),
        }));
      }
    },
    [
      gateway,
      indexGeneration,
      projectId,
      state.caseSensitive,
      state.wholeWord,
      state.useRegex,
      state.includePatternText,
      state.excludePatternText,
      useIgnoreFiles,
      filesExclude,
      state.pages,
      state.query,
      transitionEpoch,
    ],
  );
  const submit = useCallback(() => runSearch(false), [runSearch]);
  const continueSearch = useCallback(() => runSearch(true), [runSearch]);

  useEffect(() => {
    loadWorkspaceSearchChecks().catch(() => undefined);
  }, []);

  const submitRef = useRef(submit);
  submitRef.current = submit;
  const stateRef = useRef(state);
  stateRef.current = state;

  useEffect(() => {
    return subscribeWorkspaceNotifySearch((notice) => {
      if (
        notice.projectId !== projectId ||
        notice.projectGeneration !== indexGeneration ||
        notice.transitionEpoch !== transitionEpoch ||
        stateRef.current.phase !== "results"
      ) {
        return;
      }
      void submitRef.current();
    });
  }, [indexGeneration, projectId, transitionEpoch]);

  return {
    state,
    ...setters,
    submit,
    continueSearch,
    cancel,
    refresh: submit,
  };
}
import { errorMessage } from "../../domain/errors";
