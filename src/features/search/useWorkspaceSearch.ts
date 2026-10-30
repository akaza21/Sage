import { useCallback, useEffect, useRef, useState } from "react";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { subscribeWorkspaceNotifySearch } from "../files/workspaceNotifyCompose";
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

function filterPatterns(value: string): string[] {
  return value
    .split(",")
    .map((pattern) => pattern.trim())
    .filter(Boolean);
}

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

  // Turning `search.useIgnoreFiles` on or off retires results like any other
  // criterion, so a page from one mode never continues in the other.
  useEffect(
    () =>
      useEditorSettingsStore.subscribe((store, previous) => {
        const key = "search.useIgnoreFiles";
        if (store.settings[key] === previous.settings[key]) return;
        retireCriteria();
        setState((current) => ({
          ...current,
          phase: "idle",
          response: null,
          pages: [],
          error: null,
        }));
      }),
    [retireCriteria],
  );

  const setQuery = useCallback(
    (query: string) => {
      retireCriteria();
      setState((current) => ({
        ...current,
        phase: "idle",
        query,
        response: null,
        pages: [],
        error: null,
      }));
    },
    [retireCriteria],
  );

  const setCaseSensitive = useCallback(
    (caseSensitive: boolean) => {
      retireCriteria();
      setState((current) => ({
        ...current,
        phase: "idle",
        caseSensitive,
        response: null,
        pages: [],
        error: null,
      }));
    },
    [retireCriteria],
  );

  const setWholeWord = useCallback(
    (wholeWord: boolean) => {
      retireCriteria();
      setState((current) => ({
        ...current,
        phase: "idle",
        wholeWord,
        response: null,
        pages: [],
        error: null,
      }));
    },
    [retireCriteria],
  );

  const setIncludePatternText = useCallback(
    (includePatternText: string) => {
      retireCriteria();
      setState((current) => ({
        ...current,
        phase: "idle",
        includePatternText,
        response: null,
        pages: [],
        error: null,
      }));
    },
    [retireCriteria],
  );

  const setExcludePatternText = useCallback(
    (excludePatternText: string) => {
      retireCriteria();
      setState((current) => ({
        ...current,
        phase: "idle",
        excludePatternText,
        response: null,
        pages: [],
        error: null,
      }));
    },
    [retireCriteria],
  );

  const cancel = useCallback(() => {
    generation.current += 1;
    active.current?.abort();
    active.current = null;
    setState((current) => ({
      ...current,
      phase: "idle",
      response: null,
      pages: [],
      error: null,
    }));
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
          includePatterns: filterPatterns(state.includePatternText),
          excludePatterns: filterPatterns(state.excludePatternText),
          // Sent only when off, so the native default (on) needs no field.
          ...(useIgnoreFiles ? {} : { useIgnoreFiles: false }),
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
          includePatterns: filterPatterns(state.includePatternText),
          excludePatterns: filterPatterns(state.excludePatternText),
          // Sent only when off, so the native default (on) needs no field.
          ...(useIgnoreFiles ? {} : { useIgnoreFiles: false }),
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
