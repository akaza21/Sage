import { Search, X } from "../../ui/leanIcons";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type RefObject,
} from "react";

import { chatPlacement, openChat } from "../../app/openChat";
import { useWorkbenchStore } from "../../app/store";
import { boundedError } from "../../domain/errors";
import { ModalDialog } from "../../ui/ModalDialog";
import { SearchablePicker } from "../../ui/SearchablePicker";
import { projectChatSearchDesktop } from "./chatSearchDesktop";
import {
  admitChatSearchFolder,
  chatSearchFolderOptions,
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

import { ChatSearchResults } from "./ChatSearchResults";

type SearchPhase = "idle" | "searching" | "results" | "error";

/** The live status: progress, then how many Chats were found. */
function searchStatus(
  phase: SearchPhase,
  response: ChatSearchFanoutView | null,
): string {
  if (phase === "searching") return "Searching…";
  if (phase !== "results" || !response) return "";
  const count = response.results.length;
  return `${count}${response.hasMore ? "+" : ""} ${count === 1 ? "chat" : "chats"}`;
}

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
  const projects = useWorkbenchStore((state) => state.projects);
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<ChatSearchScope | null>(null);
  const [query, setQuery] = useState("");
  const [phase, setPhase] = useState<SearchPhase>("idle");
  const [response, setResponse] = useState<ChatSearchFanoutView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestGeneration = useRef(0);
  const searching = useRef(false);
  const [settledRequest, setSettledRequest] = useState(0);
  const queryRef = useRef<HTMLInputElement>(null);
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
    requestGeneration.current += 1;
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
    queryRef.current?.focus();
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

  const searchQuery = useCallback(async () => {
    if (!scope || searching.current) return;
    const normalized = query.trim();
    if (!matchesScope(scope)) {
      close(false);
      return;
    }
    const intent = requestGeneration.current + 1;
    requestGeneration.current = intent;
    searching.current = true;
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
    } finally {
      searching.current = false;
      setSettledRequest((value) => value + 1);
    }
  }, [close, query, scope]);

  useEffect(() => {
    if (!open || phase !== "idle") return;
    const timer = setTimeout(() => void searchQuery(), 200);
    return () => clearTimeout(timer);
  }, [open, phase, query, searchQuery, settledRequest]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    void searchQuery();
  };

  const selectResult = async (result: ChatSearchFanoutPick) => {
    if (!scope || !matchesScope(scope)) {
      close(false);
      return;
    }
    const state = useWorkbenchStore.getState();
    const { attached, archived } = chatPlacement(
      state,
      result.projectId,
      result.chatId,
    );
    if (
      result.projectId === state.project?.id &&
      (result.attached !== attached || (!attached && !archived))
    ) {
      requestGeneration.current += 1;
      setResponse(null);
      setPhase("error");
      setError("This Chat result changed. Search again to continue.");
      return;
    }
    close(false);
    await openChat(result.projectId, result.chatId, {
      attached: result.attached,
      revealRestored: false,
    });
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
          initialFocusRef={queryRef}
          onDismiss={() => close()}
        >
          <form className="chat-search-dialog__form" onSubmit={submit}>
            <h2 id="chat-search-title">Search Chats</h2>
            {/* Dismiss sits in the header, as in the other dialogs, so it
                stays put while results and errors come and go. */}
            <button
              className="icon-button chat-search-dialog__close"
              type="button"
              aria-label="Close"
              title="Close"
              onClick={() => close()}
            >
              <X size={16} aria-hidden="true" />
            </button>
            <p>
              Search durable titles and primary conversation text in the
              selected folder, or across listed folders.
            </p>
            <label className="chat-search-dialog__query">
              <span>Search Chats</span>
              <input
                ref={queryRef}
                autoFocus
                aria-label="Search Chats"
                disabled={projectTransitioning}
                maxLength={MAX_CHAT_SEARCH_QUERY_BYTES}
                role="searchbox"
                type="search"
                value={query}
                onChange={(event) => editQuery(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key !== "ArrowDown") return;
                  const result = event.currentTarget
                    .closest("form")
                    ?.querySelector<HTMLButtonElement>('[role="option"]');
                  if (result) {
                    event.preventDefault();
                    result.focus();
                  }
                }}
              />
            </label>
            <label className="chat-search-dialog__query">
              <span>Search in</span>
              <SearchablePicker
                ariaLabel="Search in"
                candidates={folderCandidates}
                disabled={projectTransitioning}
                onSelect={chooseFolder}
                portalTarget="closest-dialog"
                selectedId={scope.searchProjectId}
              />
            </label>
            <p className="chat-search-dialog__status" role="status">
              {searchStatus(phase, response)}
            </p>
            <ChatSearchResults
              response={phase === "results" ? response : null}
              error={error}
              scope={scope}
              folders={folders}
              onFocusQuery={() => queryRef.current?.focus()}
              onSelect={selectResult}
            />
          </form>
        </ModalDialog>
      ) : null}
    </>
  );
}
