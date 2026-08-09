import { useCallback, useEffect, useRef, useState } from "react";

import type {
  WorkspaceSearchGateway,
  WorkspaceSearchResponse,
} from "./workspaceSearchDomain";

export type WorkspaceSearchPhase = "idle" | "searching" | "results" | "error";

export interface WorkspaceSearchState {
  phase: WorkspaceSearchPhase;
  query: string;
  caseSensitive: boolean;
  response: WorkspaceSearchResponse | null;
  error: string | null;
}

interface UseWorkspaceSearchOptions {
  projectId: string | null;
  gateway: WorkspaceSearchGateway;
}

const initialState = (): WorkspaceSearchState => ({
  phase: "idle",
  query: "",
  caseSensitive: false,
  response: null,
  error: null,
});

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useWorkspaceSearch({
  projectId,
  gateway,
}: UseWorkspaceSearchOptions) {
  const [state, setState] = useState<WorkspaceSearchState>(initialState);
  const active = useRef<AbortController | null>(null);
  const generation = useRef(0);

  useEffect(() => {
    generation.current += 1;
    active.current?.abort();
    active.current = null;
    setState(initialState());
    return () => active.current?.abort();
  }, [projectId]);

  const setQuery = useCallback((query: string) => {
    setState((current) => ({ ...current, query, error: null }));
  }, []);

  const setCaseSensitive = useCallback((caseSensitive: boolean) => {
    setState((current) => ({ ...current, caseSensitive, error: null }));
  }, []);

  const cancel = useCallback(() => {
    generation.current += 1;
    active.current?.abort();
    active.current = null;
    setState((current) => ({
      ...current,
      phase: "idle",
      response: null,
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
        { projectId, query, caseSensitive: state.caseSensitive },
        controller.signal,
      );
      if (
        controller.signal.aborted ||
        generation.current !== requestGeneration
      ) {
        return;
      }
      active.current = null;
      setState((current) => ({
        ...current,
        phase: "results",
        response,
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
  }, [gateway, projectId, state.caseSensitive, state.query]);

  return { state, setQuery, setCaseSensitive, submit, cancel };
}
