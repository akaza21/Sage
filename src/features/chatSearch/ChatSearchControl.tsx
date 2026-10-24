import { Search } from "../../ui/leanIcons";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type RefObject,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import { boundedError } from "../../domain/errors";
import { ModalDialog } from "../../ui/ModalDialog";
import { SearchablePicker } from "../../ui/SearchablePicker";
import { projectChatSearchDesktop } from "./chatSearchDesktop";
import {
  admitChatSearchFolder,
  chatSearchAttachment,
  chatSearchFolderOptions,
  chatSearchResultFolderName,
  isChatSearchFanout,
  matchesChatSearchScope,
  runScopedChatSearch,
  type ChatSearchFanoutPick,
  type ChatSearchFanoutView,
} from "./chatSearchFanout";
import {
  MAX_CHAT_SEARCH_QUERY_BYTES,
  requestChatSearchInFolder,
  subscribeChatSearchFolderRequest,
  takeChatSearchFocusReturn,
  takeChatSearchFolderRequest,
  type ChatSearchScope,
} from "./chatSearchScope";

type SearchPhase = "idle" | "searching" | "results" | "error";

function matchesScope(scope: ChatSearchScope) {
  return matchesChatSearchScope(useWorkbenchStore.getState(), scope);
}

export function ChatSearchTrigger({
  triggerRef,
}: {
  triggerRef?: RefObject<HTMLButtonElement | null>;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  return (
    <button
      ref={triggerRef}
      className="project-action"
      type="button"
      title="Search chats"
      aria-label="Search chats"
      disabled={!project || transitioning}
      onClick={(event) => {
        if (!project || transitioning) return;
        const trigger = event.currentTarget;
        requestChatSearchInFolder(project.id, () => trigger.focus());
      }}
    >
      <Search size={16} aria-hidden="true" />
      Search chats
    </button>
  );
}

export function ChatSearchControl({
  showTrigger = true,
  triggerRef: suppliedTriggerRef,
}: {
  showTrigger?: boolean;
  triggerRef?: RefObject<HTMLButtonElement | null>;
} = {}) {
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const activateChat = useWorkbenchStore((state) => state.activateChat);
  const openRailChat = useWorkbenchStore((state) => state.openRailChat);
  const restoreChat = useWorkbenchStore((state) => state.restoreChat);
  const projects = useWorkbenchStore((state) => state.projects);
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<ChatSearchScope | null>(null);
  const [query, setQuery] = useState("");
  const [phase, setPhase] = useState<SearchPhase>("idle");
  const [response, setResponse] = useState<ChatSearchFanoutView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestGeneration = useRef(0);
  const localTriggerRef = useRef<HTMLButtonElement | null>(null);
  const triggerRef = suppliedTriggerRef ?? localTriggerRef;
  const focusReturnRef = useRef<(() => void) | null>(null);

  const close = useCallback(
    (restoreFocus = true) => {
      requestGeneration.current += 1;
      setOpen(false);
      setScope(null);
      setQuery("");
      setPhase("idle");
      setResponse(null);
      setError(null);
      const focusReturn = focusReturnRef.current;
      focusReturnRef.current = null;
      if (restoreFocus) {
        setTimeout(() => {
          if (focusReturn) focusReturn();
          else triggerRef.current?.focus();
        }, 0);
      }
    },
    [triggerRef],
  );

  useEffect(() => {
    if (!open || !scope) return;
    return useWorkbenchStore.subscribe((state) => {
      if (!matchesChatSearchScope(state, scope)) {
        close();
      }
    });
  }, [close, open, scope]);

  const openSearch = (
    searchProjectId?: string,
    focusReturn?: (() => void) | null,
  ) => {
    const state = useWorkbenchStore.getState();
    if (!state.project || state.projectTransitioning) return;
    focusReturnRef.current = focusReturn ?? null;
    setScope({
      activeProjectId: state.project.id,
      searchProjectId: admitChatSearchFolder(
        chatSearchFolderOptions(state.projects),
        searchProjectId,
        state.project.id,
      ),
      projectGeneration: state.projectGeneration,
      transitionEpoch: state.nativeTransitionEpoch,
    });
    setQuery("");
    setPhase("idle");
    setResponse(null);
    setError(null);
    setOpen(true);
  };

  useEffect(() => {
    const consumeRequest = () => {
      const folderId = takeChatSearchFolderRequest();
      if (folderId) {
        openSearch(folderId, takeChatSearchFocusReturn());
      }
    };
    const unsubscribe = subscribeChatSearchFolderRequest(consumeRequest);
    consumeRequest();
    return unsubscribe;
  }, []);

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
      const result = await runScopedChatSearch({
        searchProjectId: scope.searchProjectId,
        folders: chatSearchFolderOptions(useWorkbenchStore.getState().projects),
        projectGeneration: scope.projectGeneration,
        expectedTransitionEpoch: scope.transitionEpoch,
        query: normalized,
        search: (request) => projectChatSearchDesktop.search(request),
      });
      if (requestGeneration.current !== intent || !matchesScope(scope)) return;
      setResponse(result);
      setPhase("results");
    } catch (searchError) {
      if (requestGeneration.current !== intent || !matchesScope(scope)) return;
      setError(boundedError(searchError, 2_048, "utf8"));
      setPhase("error");
    }
  };

  const selectResult = async (result: ChatSearchFanoutPick) => {
    if (!scope || !matchesScope(scope)) {
      close(false);
      return;
    }
    const state = useWorkbenchStore.getState();
    const { attached, archived } = chatSearchAttachment(
      state,
      result.projectId,
      result.chatId,
    );
    if (result.attached !== attached || (!attached && !archived)) {
      requestGeneration.current += 1;
      setResponse(null);
      setPhase("error");
      setError("This Chat result changed. Search again to continue.");
      return;
    }
    close(false);
    if (attached) {
      if (result.projectId === state.project?.id) {
        await activateChat(result.chatId);
      } else {
        await openRailChat(result.projectId, result.chatId);
      }
      return;
    }
    await restoreChat(result.chatId, result.projectId);
  };

  const chooseFolder = useCallback(
    (searchProjectId: string) => {
      if (!scope || searchProjectId === scope.searchProjectId) return;
      requestGeneration.current += 1;
      setScope({ ...scope, searchProjectId });
      setPhase("idle");
      setResponse(null);
      setError(null);
    },
    [scope],
  );
  const folders = useMemo(() => chatSearchFolderOptions(projects), [projects]);
  const folderCandidates = useMemo(
    () => folders.map((folder) => ({ id: folder.id, label: folder.name })),
    [folders],
  );

  return (
    <>
      {showTrigger ? <ChatSearchTrigger triggerRef={triggerRef} /> : null}
      {open && scope ? (
        <ModalDialog
          className="chat-search-dialog"
          labelledBy="chat-search-title"
          onDismiss={() => close()}
        >
          <form className="chat-search-dialog__form" onSubmit={submit}>
            <h2 id="chat-search-title">Search Chats</h2>
            <p>
              Search durable titles and primary conversation text in the
              selected folder, or across listed folders.
            </p>
            <label className="chat-search-dialog__query">
              <span>Search in</span>
              <SearchablePicker
                ariaLabel="Search in"
                candidates={folderCandidates}
                disabled={phase === "searching" || projectTransitioning}
                onSelect={chooseFolder}
                portalTarget="closest-dialog"
                selectedId={scope.searchProjectId}
              />
            </label>
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
                    {response.results.map((result) => {
                      const folderName = chatSearchResultFolderName(
                        folders,
                        result.projectId,
                      );
                      const showFolder =
                        Boolean(folderName) &&
                        (isChatSearchFanout(scope.searchProjectId) ||
                          result.projectId !== scope.activeProjectId);
                      return (
                        <button
                          className="chat-search-result"
                          key={`${result.projectId}:${result.chatId}`}
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
                              {showFolder ? ` · ${folderName}` : ""}
                            </span>
                          </span>
                          {result.snippet ? (
                            <span>{result.snippet}</span>
                          ) : null}
                        </button>
                      );
                    })}
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
        </ModalDialog>
      ) : null}
    </>
  );
}
