import { Search } from "lucide-react";
import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import {
  MAX_CHAT_SEARCH_QUERY_BYTES,
  validateProjectChatSearchRequest,
  type ProjectChatSearchResponse,
  type ProjectChatSearchResult,
} from "./chatSearchDomain";
import { projectChatSearchDesktop } from "./chatSearchDesktop";

interface SearchScope {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
}

type SearchPhase = "idle" | "searching" | "results" | "error";

function matchesScope(scope: SearchScope) {
  const state = useWorkbenchStore.getState();
  return (
    !state.projectTransitioning &&
    state.project?.id === scope.projectId &&
    state.projectGeneration === scope.projectGeneration &&
    state.nativeTransitionEpoch === scope.transitionEpoch
  );
}

function boundedError(error: unknown): string {
  const value = error instanceof Error ? error.message : String(error);
  const bytes = new TextEncoder().encode(value);
  if (bytes.length <= 2_048) return value;
  let end = 2_048;
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) end -= 1;
  return new TextDecoder().decode(bytes.slice(0, end));
}

export function ChatSearchControl() {
  const project = useWorkbenchStore((state) => state.project);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const activateChat = useWorkbenchStore((state) => state.activateChat);
  const restoreChat = useWorkbenchStore((state) => state.restoreChat);
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<SearchScope | null>(null);
  const [query, setQuery] = useState("");
  const [phase, setPhase] = useState<SearchPhase>("idle");
  const [response, setResponse] = useState<ProjectChatSearchResponse | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const requestGeneration = useRef(0);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const dialogRef = useRef<HTMLDialogElement | null>(null);

  const close = (restoreFocus = true) => {
    requestGeneration.current += 1;
    setOpen(false);
    setScope(null);
    setQuery("");
    setPhase("idle");
    setResponse(null);
    setError(null);
    if (restoreFocus) queueMicrotask(() => triggerRef.current?.focus());
  };

  useEffect(() => {
    if (!open || !scope) return;
    return useWorkbenchStore.subscribe((state) => {
      if (
        state.projectTransitioning ||
        state.project?.id !== scope.projectId ||
        state.projectGeneration !== scope.projectGeneration ||
        state.nativeTransitionEpoch !== scope.transitionEpoch
      ) {
        close(false);
      }
    });
  }, [open, scope]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!open || !dialog) return;
    if (typeof dialog.showModal === "function") {
      if (!dialog.open) dialog.showModal();
    } else {
      dialog.setAttribute("open", "");
    }
    return () => {
      if (typeof dialog.close === "function" && dialog.open) {
        dialog.close();
      } else {
        dialog.removeAttribute("open");
      }
    };
  }, [open]);

  const openSearch = () => {
    if (!project || projectTransitioning) return;
    setScope({
      projectId: project.id,
      projectGeneration,
      transitionEpoch,
    });
    setQuery("");
    setPhase("idle");
    setResponse(null);
    setError(null);
    setOpen(true);
  };

  const editQuery = (value: string) => {
    requestGeneration.current += 1;
    setQuery(value.slice(0, MAX_CHAT_SEARCH_QUERY_BYTES));
    setPhase("idle");
    setResponse(null);
    setError(null);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!scope || phase === "searching") return;
    const normalized = query.trim();
    if (!normalized) {
      setPhase("error");
      setError("Enter text to search for.");
      return;
    }
    const request = {
      projectId: scope.projectId,
      projectGeneration: scope.projectGeneration,
      expectedTransitionEpoch: scope.transitionEpoch,
      query: normalized,
    };
    try {
      validateProjectChatSearchRequest(request);
    } catch (validationError) {
      setPhase("error");
      setError(boundedError(validationError));
      return;
    }
    if (!matchesScope(scope)) {
      close(false);
      return;
    }
    const intent = requestGeneration.current + 1;
    requestGeneration.current = intent;
    setQuery(normalized);
    setPhase("searching");
    setResponse(null);
    setError(null);
    try {
      const result = await projectChatSearchDesktop.search(request);
      if (requestGeneration.current !== intent || !matchesScope(scope)) return;
      setResponse(result);
      setPhase("results");
    } catch (searchError) {
      if (requestGeneration.current !== intent || !matchesScope(scope)) return;
      setError(boundedError(searchError));
      setPhase("error");
    }
  };

  const selectResult = async (result: ProjectChatSearchResult) => {
    if (!scope || !matchesScope(scope)) {
      close(false);
      return;
    }
    const state = useWorkbenchStore.getState();
    const attached = state.tabs.some(
      (tab) => tab.kind === "chat" && tab.id === result.chatId,
    );
    const archived = state.archivedChats.some(
      (chat) => chat.id === result.chatId,
    );
    if (result.attached !== attached || (!attached && !archived)) {
      requestGeneration.current += 1;
      setResponse(null);
      setPhase("error");
      setError("This Chat result changed. Search again to continue.");
      return;
    }
    close();
    if (attached) {
      await activateChat(result.chatId);
    } else {
      await restoreChat(result.chatId);
    }
  };

  const containFocus = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>(
        'input:not([disabled]), button:not([disabled]), [role="option"]:not([aria-disabled="true"])',
      ),
    );
    const first = focusable[0];
    const last = focusable.at(-1);
    if (!first || !last) {
      event.preventDefault();
    } else if (
      event.shiftKey &&
      (document.activeElement === first ||
        !event.currentTarget.contains(document.activeElement))
    ) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <>
      <button
        ref={triggerRef}
        className="project-action"
        type="button"
        title="Search chats"
        aria-label="Search chats"
        disabled={!project || projectTransitioning}
        onClick={openSearch}
      >
        <Search size={16} aria-hidden="true" />
        Search chats
      </button>
      {open && scope ? (
        <dialog
          ref={dialogRef}
          className="dirty-dialog chat-search-dialog"
          aria-modal="true"
          aria-labelledby="chat-search-title"
          onCancel={(event) => {
            event.preventDefault();
            close();
          }}
          onKeyDown={containFocus}
        >
          <form className="chat-search-dialog__form" onSubmit={submit}>
            <h2 id="chat-search-title">Search Chats</h2>
            <p>
              Search durable titles and primary conversation text in this
              Project.
            </p>
            <label className="chat-search-dialog__query">
              <span>Search Chats</span>
              <input
                autoFocus
                aria-label="Search Chats"
                disabled={phase === "searching" || projectTransitioning}
                maxLength={MAX_CHAT_SEARCH_QUERY_BYTES}
                role="searchbox"
                type="search"
                value={query}
                onChange={(event) => editQuery(event.target.value)}
              />
            </label>
            <div className="chat-search-dialog__actions">
              <button className="button" type="button" onClick={() => close()}>
                Cancel
              </button>
              <button
                className="button button--primary"
                type="submit"
                disabled={phase === "searching" || projectTransitioning}
              >
                {phase === "searching" ? "Searching…" : "Search"}
              </button>
            </div>
            {error ? (
              <div className="composer__error" role="alert">
                {error}
              </div>
            ) : null}
            {phase === "results" && response ? (
              <div className="chat-search-dialog__results">
                {response.results.length === 0 ? (
                  <p className="chat-search-dialog__empty">
                    No Chats matched “{response.query}”.
                  </p>
                ) : (
                  <div role="listbox" aria-label="Chat search results">
                    {response.results.map((result) => (
                      <button
                        className="chat-search-result"
                        key={result.chatId}
                        type="button"
                        role="option"
                        aria-selected="false"
                        aria-label={`${result.title} — ${result.attached ? "Attached" : "Archived"}`}
                        onClick={() => void selectResult(result)}
                      >
                        <span className="chat-search-result__heading">
                          <strong>{result.title}</strong>
                          <span>
                            {result.attached ? "Attached" : "Archived"}
                          </span>
                        </span>
                        {result.snippet ? <span>{result.snippet}</span> : null}
                      </button>
                    ))}
                  </div>
                )}
                {response.hasMore ? (
                  <p className="chat-search-dialog__more">
                    More matches exist. Refine the query to narrow the results.
                  </p>
                ) : null}
              </div>
            ) : null}
          </form>
        </dialog>
      ) : null}
    </>
  );
}
