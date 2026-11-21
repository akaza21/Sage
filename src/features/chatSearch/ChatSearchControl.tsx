import { Search } from "../../ui/leanIcons";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import { chatPlacement, openChat } from "../../app/openChat";
import { useWorkbenchStore } from "../../app/store";
import {
  openProjectCreation,
  useProjectCreationState,
} from "../../app/projectCreationState";
import { boundedError } from "../../domain/errors";
import { projectChatSearchDesktop } from "./chatSearchDesktop";
import {
  CHAT_SEARCH_ALL_FOLDERS_ID,
  admitChatSearchFolder,
  chatSearchFolderOptions,
  matchesChatSearchScope,
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

import { lazyParts } from "../chat/lazyParts";
import type * as PaletteChunk from "./ChatSearchPalette";
const PalettePart = lazyParts<typeof PaletteChunk>(
  () => import("./ChatSearchPalette"),
);
type SearchPhase = "idle" | "searching" | "results" | "error";

/** Announce zero results here; the visible empty-state message is not a live region. */
function searchStatus(
  phase: SearchPhase,
  response: ChatSearchFanoutView | null,
): string {
  if (phase === "searching") return "Searching…";
  if (phase !== "results" || !response) return "";
  const count = response.results.length;
  const more = response.hasMore;
  return `${response.errors.length ? "Partial results: " : ""}${count}${more ? "+" : ""} ${count === 1 && !more ? "chat" : "chats"}`;
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
        requestChatSearchInFolder(CHAT_SEARCH_ALL_FOLDERS_ID, () =>
          trigger.focus(),
        );
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
  const unscopedChats = useWorkbenchStore((state) => state.unscopedChats);
  const [open, setOpen] = useState(false);
  const [searchEnabled, setSearchEnabled] = useState(true);
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
        close(!useProjectCreationState.getState().open);
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
        state.project.id === searchProjectId
          ? state.project.id
          : CHAT_SEARCH_ALL_FOLDERS_ID,
      ),
      projectGeneration: state.projectGeneration,
      transitionEpoch: state.nativeTransitionEpoch,
    });
    setQuery("");
    setPhase("idle");
    setResponse(null);
    setError(null);
    setSearchEnabled(true);
    setOpen(true);
    void import("./chatSearchExecution").catch(() => undefined);
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
    if (!scope || !searchEnabled || searching.current) return;
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
      const { runScopedChatSearch } = await import("./chatSearchExecution");
      const result = await runScopedChatSearch({
        searchProjectId: scope.searchProjectId,
        folders: chatSearchFolderOptions(useWorkbenchStore.getState().projects),
        privateFolders: useWorkbenchStore.getState().unscopedChats,
        projectGeneration: scope.projectGeneration,
        expectedTransitionEpoch: scope.transitionEpoch,
        query: normalized,
        isCurrent: () =>
          requestGeneration.current === intent && matchesScope(scope),
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
  }, [close, query, scope, searchEnabled]);

  useEffect(() => {
    if (!open || !searchEnabled || phase !== "idle") return;
    const timer = setTimeout(() => void searchQuery(), 200);
    return () => clearTimeout(timer);
  }, [open, phase, query, searchQuery, settledRequest, searchEnabled]);

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
      setError("This chat result changed. Search again to continue.");
      return;
    }
    close(false);
    await openChat(result.projectId, result.chatId, {
      attached: result.attached,
      revealRestored: false,
    });
  };

  const folders = useMemo(
    () => chatSearchFolderOptions(projects, unscopedChats.length > 0),
    [projects, unscopedChats],
  );
  return (
    <>
      {showTrigger ? <ChatSearchTrigger triggerRef={triggerRef} /> : null}
      {open && scope ? (
        <PalettePart
          part="ChatSearchPalette"
          queryRef={queryRef}
          query={query}
          disabled={projectTransitioning}
          response={phase === "results" ? response : null}
          status={searchStatus(phase, response)}
          error={error}
          folders={folders}
          searchProjectId={scope.searchProjectId}
          onScope={(searchProjectId) => {
            requestGeneration.current += 1;
            setScope({ ...scope, searchProjectId });
            setPhase("idle");
            setResponse(null);
            setError(null);
          }}
          onQuery={editQuery}
          onSearchEnabled={(enabled) => {
            if (enabled === searchEnabled) return;
            requestGeneration.current += 1;
            setSearchEnabled(enabled);
            setPhase("idle");
          }}
          onSearch={searchQuery}
          onSelect={selectResult}
          onClose={close}
          onCreateProject={openProjectCreation}
        />
      ) : null}
    </>
  );
}
