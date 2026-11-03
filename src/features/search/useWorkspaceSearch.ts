import { useCallback, useEffect, useRef, useState } from "react";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { utf8ByteLength } from "../../domain/text";
import { subscribeWorkspaceNotifySearch } from "../files/workspaceNotifyCompose";
import { sameSearchSettings, searchSettingFields } from "./searchSettings";
import type {
  WorkspaceSearchGateway,
  WorkspaceSearchResponse,
} from "./workspaceSearchDomain";
import { parseWorkspaceSearchResponse } from "./workspaceSearchDomain";
import { WORKSPACE_SEARCH_LIMITS } from "./workspaceSearchLimits";

export type WorkspaceSearchPhase = "idle" | "searching" | "results" | "error";

export interface WorkspaceSearchState {
  phase: WorkspaceSearchPhase;
  query: string;
  caseSensitive: boolean;
  wholeWord: boolean;
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
  includePatternText: "",
  excludePatternText: "",
  response: null,
  pages: [],
  error: null,
});

/**
 * Splits a field on the commas that sit outside `{…}`; a field whose braces
 * do not balance splits on every comma.
 */
function splitFilterField(value: string): string[] {
  const pieces: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < value.length; index += 1) {
    const char = value[index];
    if (char === "{") depth += 1;
    else if (char === "}" && depth > 0) depth -= 1;
    else if (char === "," && depth === 0) {
      pieces.push(value.slice(start, index));
      start = index + 1;
    }
  }
  pieces.push(value.slice(start));
  return (depth === 0 ? pieces : value.split(","))
    .map((piece) => piece.trim())
    .filter(Boolean);
}

/**
 * Expands each `{a,b}` group that holds no other brace; others stay literal.
 * Stops once the distinct results pass `limit`, so a long field cannot grow
 * without bound before the caller refuses it.
 */
function expandBraces(piece: string, limit: number): string[] {
  const group = /\{([^{}]*)\}/.exec(piece);
  if (!group) return [piece];
  const head = piece.slice(0, group.index);
  const tail = expandBraces(piece.slice(group.index + group[0].length), limit);
  const results = new Set<string>();
  for (const choice of group[1].split(",")) {
    for (const rest of tail) {
      results.add(head + choice + rest);
      if (results.size > limit) return [...results];
    }
  }
  return [...results];
}

/**
 * Reads one include or exclude piece as VS Code does, in Sage's whole-path
 * dialect: a name with no `/` matches at any depth (and, without wildcards,
 * everything below it too), a trailing `/` means the folder's contents and a
 * leading `./` anchors to the root. A piece that names a path is unchanged.
 */
function expandFilterPiece(piece: string): string[] {
  let pattern = piece;
  let anchored = false;
  while (pattern.startsWith("./")) {
    pattern = pattern.slice(2);
    anchored = true;
  }
  if (pattern.endsWith("/")) {
    pattern = pattern.replace(/\/+$/, "");
    return pattern ? [`${pattern}/**`] : [];
  }
  if (!pattern) return [];
  if (pattern.includes("/") || pattern === "**") return [pattern];
  const prefixed = anchored ? pattern : `**/${pattern}`;
  return /[*?]/.test(pattern) ? [prefixed] : [prefixed, `${prefixed}/**`];
}

function filterPatterns(value: string): string[] {
  const limit = WORKSPACE_SEARCH_LIMITS.filterPatterns;
  const patterns = new Set<string>();
  for (const piece of splitFilterField(value)) {
    for (const choice of expandBraces(piece, limit)) {
      for (const pattern of expandFilterPiece(choice.trim())) {
        patterns.add(pattern);
        // One past the limit is enough for the caller to refuse the field.
        if (patterns.size > limit) return [...patterns];
      }
    }
  }
  return [...patterns];
}

/**
 * Both expanded lists. Throws the request validator's invalid-filter message
 * when they exceed the native limits, so a partial set is never sent.
 */
function searchFilters(includeText: string, excludeText: string) {
  const includePatterns = filterPatterns(includeText);
  const excludePatterns = filterPatterns(excludeText);
  const { filterPatterns: maxPatterns, filterBytes } = WORKSPACE_SEARCH_LIMITS;
  if (
    includePatterns.length > maxPatterns ||
    excludePatterns.length > maxPatterns ||
    utf8ByteLength([...includePatterns, ...excludePatterns].join("")) >
      filterBytes
  ) {
    throw new Error("workspace Search path filters are invalid");
  }
  return { includePatterns, excludePatterns };
}

type CriterionKey =
  | "query"
  | "caseSensitive"
  | "wholeWord"
  | "includePatternText"
  | "excludePatternText";

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

  useEffect(() => {
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

  // Changing `search.useIgnoreFiles` or `files.exclude` retires results like
  // any other criterion, so a page never continues under other settings.
  useEffect(
    () =>
      useEditorSettingsStore.subscribe((store, previous) => {
        if (sameSearchSettings(store.settings, previous.settings)) return;
        retireCriteria();
        setState((current) => ({ ...current, ...clearedResults }));
      }),
    [retireCriteria],
  );

  /** Each criterion change retires the shown results and any pending scan. */
  const setCriterion = useCallback(
    <K extends CriterionKey>(key: K, value: WorkspaceSearchState[K]) => {
      retireCriteria();
      setState((current) => ({ ...current, ...clearedResults, [key]: value }));
    },
    [retireCriteria],
  );
  const setQuery = useCallback(
    (query: string) => setCriterion("query", query),
    [setCriterion],
  );
  const setCaseSensitive = useCallback(
    (caseSensitive: boolean) => setCriterion("caseSensitive", caseSensitive),
    [setCriterion],
  );
  const setWholeWord = useCallback(
    (wholeWord: boolean) => setCriterion("wholeWord", wholeWord),
    [setCriterion],
  );
  const setIncludePatternText = useCallback(
    (text: string) => setCriterion("includePatternText", text),
    [setCriterion],
  );
  const setExcludePatternText = useCallback(
    (text: string) => setCriterion("excludePatternText", text),
    [setCriterion],
  );

  const cancel = useCallback(() => {
    generation.current += 1;
    active.current?.abort();
    active.current = null;
    setState((current) => ({ ...current, ...clearedResults }));
  }, []);

  const submit = useCallback(async () => {
    const query = state.query.trim();
    if (!projectId) {
      setState((current) => ({
        ...current,
        phase: "error",
        response: null,
        error: "Open a Project before searching.",
      }));
      return;
    }
    if (!query) {
      setState((current) => ({
        ...current,
        phase: "error",
        response: null,
        error: "Enter text to search for.",
      }));
      return;
    }

    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
    const requestGeneration = generation.current + 1;
    generation.current = requestGeneration;
    setState((current) => ({
      ...current,
      phase: "searching",
      query,
      response: null,
      error: null,
    }));

    try {
      const response = await gateway.search(
        {
          projectId,
          expectedTransitionEpoch: transitionEpoch,
          indexGeneration,
          continuationToken: null,
          query,
          caseSensitive: state.caseSensitive,
          wholeWord: state.wholeWord,
          ...searchFilters(state.includePatternText, state.excludePatternText),
          ...searchSettingFields(useIgnoreFiles, filesExclude),
        },
        controller.signal,
      );
      if (
        controller.signal.aborted ||
        generation.current !== requestGeneration
      ) {
        return;
      }
      const validated = parseWorkspaceSearchResponse(response);
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
        pages: [validated],
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
        phase: "error",
        response: null,
        error: errorMessage(error),
      }));
    }
  }, [
    gateway,
    indexGeneration,
    projectId,
    state.caseSensitive,
    state.wholeWord,
    state.includePatternText,
    state.excludePatternText,
    useIgnoreFiles,
    filesExclude,
    state.query,
    transitionEpoch,
  ]);

  const continueSearch = useCallback(async () => {
    const lastPage = state.pages.at(-1);
    const query = state.query.trim();
    if (
      !projectId ||
      !query ||
      !lastPage?.continuationToken ||
      state.pages.length >= WORKSPACE_SEARCH_LIMITS.continuationPages
    ) {
      return;
    }

    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
    const requestGeneration = generation.current + 1;
    generation.current = requestGeneration;
    setState((current) => ({ ...current, phase: "searching", error: null }));

    try {
      const response = await gateway.search(
        {
          projectId,
          expectedTransitionEpoch: transitionEpoch,
          indexGeneration,
          continuationToken: lastPage.continuationToken,
          query,
          caseSensitive: state.caseSensitive,
          wholeWord: state.wholeWord,
          ...searchFilters(state.includePatternText, state.excludePatternText),
          ...searchSettingFields(useIgnoreFiles, filesExclude),
        },
        controller.signal,
      );
      if (
        controller.signal.aborted ||
        generation.current !== requestGeneration
      ) {
        return;
      }
      const validated = parseWorkspaceSearchResponse(response);
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
        pages: [...current.pages, validated].slice(
          0,
          WORKSPACE_SEARCH_LIMITS.continuationPages,
        ),
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
        phase: "error",
        error: errorMessage(error),
      }));
    }
  }, [
    gateway,
    indexGeneration,
    projectId,
    state.caseSensitive,
    state.wholeWord,
    state.includePatternText,
    state.excludePatternText,
    useIgnoreFiles,
    filesExclude,
    state.pages,
    state.query,
    transitionEpoch,
  ]);

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
    setQuery,
    setCaseSensitive,
    setWholeWord,
    setIncludePatternText,
    setExcludePatternText,
    submit,
    continueSearch,
    cancel,
    refresh: submit,
  };
}
import { errorMessage } from "../../domain/errors";
