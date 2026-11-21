import { NEW_PROJECT_ACTION } from "../../ui/projectPickerActions";
import {
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";
import { useWorkbenchStore } from "../../app/store";
import { SquarePen } from "../../ui/leanIcons";
import { isImeComposing } from "../../ui/imeComposition";
import {
  hasNavigationModifier,
  useActiveOption,
} from "../../ui/listNavigation";
import { ModalDialog } from "../../ui/ModalDialog";
import { shortcutLabel, usesMetaModifier } from "../../ui/shortcutLabel";
import { lazyParts } from "../chat/lazyParts";
import type * as CreateProjectChunk from "../../workbench/CreateProjectDialog";
import type * as ResultsChunk from "./ChatSearchResults";
import {
  chatSearchResultKey,
  type ChatSearchFanoutPick,
  type ChatSearchFanoutView,
} from "./chatSearchFanout";
import {
  MAX_CHAT_SEARCH_QUERY_BYTES,
  type ChatSearchFolderOption,
} from "./chatSearchScope";

const ResultsPart = lazyParts<typeof ResultsChunk>(
  () => import("./ChatSearchResults"),
);
const CreateProjectPart = lazyParts<typeof CreateProjectChunk>(
  () => import("../../workbench/CreateProjectDialog"),
);
const ACTIONS = [
  {
    id: "action:new-chat",
    label: "New chat",
    icon: SquarePen,
    shortcut: shortcutLabel("n", usesMetaModifier()),
  },
  {
    ...NEW_PROJECT_ACTION,
    icon: NEW_PROJECT_ACTION.leadingIcon,
    shortcut: null,
  },
];
type Filter = "All" | "Actions" | "Chats";
const FILTERS: Filter[] = ["All", "Actions", "Chats"];

export function ChatSearchPalette({
  queryRef,
  query,
  disabled,
  response,
  status,
  error,
  folders,
  onQuery,
  onSearchEnabled,
  onSearch,
  onSelect,
  onClose,
}: {
  queryRef: RefObject<HTMLInputElement | null>;
  query: string;
  disabled: boolean;
  response: ChatSearchFanoutView | null;
  status: string;
  error: string | null;
  folders: readonly ChatSearchFolderOption[];
  onQuery: (query: string) => void;
  onSearchEnabled: (enabled: boolean) => void;
  onSearch: () => Promise<void>;
  onSelect: (result: ChatSearchFanoutPick) => Promise<void>;
  onClose: (restoreFocus?: boolean) => void;
}) {
  const [filter, setFilter] = useState<Filter>("All");
  const [creating, setCreating] = useState(false);
  const listboxId = useId();
  const titleId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const actions = useMemo(
    () =>
      filter === "Chats"
        ? []
        : ACTIONS.filter((action) =>
            action.label.toLowerCase().includes(query.trim().toLowerCase()),
          ),
    [filter, query],
  );
  const changeFilter = (next: Filter) => {
    setFilter(next);
    onSearchEnabled(next !== "Actions");
  };
  const shown = filter === "Actions" ? null : response;
  const keys = useMemo(
    () => [
      ...actions.map((action) => action.id),
      ...(shown?.results.map(chatSearchResultKey) ?? []),
    ],
    [actions, shown],
  );
  const option = useActiveOption(keys, "none");
  const [criteria, setCriteria] = useState({ query, filter, response });
  if (
    criteria.query !== query ||
    criteria.filter !== filter ||
    criteria.response !== response
  ) {
    setCriteria({ query, filter, response });
    option.setActiveId(null);
  }
  useLayoutEffect(() => {
    if (option.activeIndex >= 0)
      document
        .getElementById(`${listboxId}-${option.activeIndex}`)
        ?.scrollIntoView?.({ block: "nearest" });
  }, [listboxId, option.activeIndex]);
  useLayoutEffect(() => {
    const center = document.querySelector(".workbench--agent .center-region");
    const panel = panelRef.current?.closest("dialog");
    if (!center || !panel) return;
    const position = () => {
      const bounds = center.getBoundingClientRect();
      panel.style.setProperty(
        "--palette-center",
        `${bounds.x + bounds.width / 2}px`,
      );
      panel.style.setProperty("--palette-available", `${bounds.width}px`);
      panel.style.setProperty(
        "--palette-top",
        `${bounds.y + bounds.height / 2}px`,
      );
    };
    position();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(position);
    observer.observe(center);
    window.addEventListener("resize", position);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", position);
    };
  }, []);
  const selectAction = (id: string) => {
    if (id === NEW_PROJECT_ACTION.id) setCreating(true);
    else {
      onClose(false);
      void useWorkbenchStore.getState().openNewChatDraft();
    }
  };
  return (
    <>
      <ModalDialog
        className="chat-search-dialog"
        labelledBy={titleId}
        initialFocusRef={queryRef}
        onPointerDown={(event) => {
          if (event.target !== event.currentTarget) return;
          const bounds = event.currentTarget.getBoundingClientRect();
          if (
            event.clientX < bounds.left ||
            event.clientX > bounds.right ||
            event.clientY < bounds.top ||
            event.clientY > bounds.bottom
          )
            onClose();
        }}
        onDismiss={() => onClose()}
      >
        <div ref={panelRef} className="chat-search-palette">
          <span id={titleId} className="sr-only">
            Search chats
          </span>
          <form
            className="chat-search-dialog__form"
            onSubmit={(event) => {
              event.preventDefault();
              if (filter !== "Actions") void onSearch();
            }}
          >
            <input
              ref={queryRef}
              autoFocus
              className="chat-search-dialog__input"
              aria-label="Search chats"
              placeholder="Search chats"
              aria-activedescendant={
                option.activeId
                  ? `${listboxId}-${option.activeIndex}`
                  : undefined
              }
              aria-autocomplete="list"
              aria-controls={listboxId}
              aria-expanded={keys.length > 0}
              disabled={disabled}
              maxLength={MAX_CHAT_SEARCH_QUERY_BYTES}
              role="combobox"
              type="text"
              value={query}
              onChange={(event) => onQuery(event.target.value)}
              onKeyDown={(event) => {
                if (
                  isImeComposing(event) ||
                  hasNavigationModifier(event) ||
                  keys.length === 0
                )
                  return;
                if (
                  ["ArrowDown", "ArrowUp", "PageDown", "PageUp"].includes(
                    event.key,
                  )
                ) {
                  event.preventDefault();
                  option.move(event.key);
                } else if (event.key === "Enter" && option.activeId) {
                  event.preventDefault();
                  const action = actions.find(
                    (action) => action.id === option.activeId,
                  );
                  if (action) selectAction(action.id);
                  else {
                    const result =
                      shown?.results[option.activeIndex - actions.length];
                    if (result) void onSelect(result);
                  }
                }
              }}
            />
            <span className="sr-only" role="status">
              {filter === "Actions" ? `${actions.length} actions` : status}
            </span>
            <div
              role="tabpanel"
              id={`${listboxId}-panel`}
              aria-labelledby={`${listboxId}-${filter}`}
              className="chat-search-dialog__body"
            >
              <div
                id={listboxId}
                role="listbox"
                aria-label="Chat search results"
              >
                {actions.length > 0 ? (
                  <div role="group" aria-label="Quick actions">
                    <h3 className="chat-search-dialog__heading">
                      Quick actions
                    </h3>
                    {actions.map((action, index) => (
                      <div
                        key={action.id}
                        id={`${listboxId}-${index}`}
                        className="chat-search-result"
                        role="option"
                        aria-label={action.label}
                        aria-selected={option.activeIndex === index}
                        onMouseDown={(event) => event.preventDefault()}
                        onMouseMove={() => option.setActiveId(action.id)}
                        onClick={() => selectAction(action.id)}
                      >
                        <action.icon size={14} aria-hidden="true" />
                        <span className="chat-search-result__title">
                          {action.label}
                        </span>
                        {action.shortcut ? (
                          <kbd className="chat-search-result__details">
                            {action.shortcut}
                          </kbd>
                        ) : null}
                      </div>
                    ))}
                  </div>
                ) : null}
                {filter !== "Actions" ? (
                  <ResultsPart
                    part="ChatSearchResults"
                    response={shown}
                    error={error}
                    folders={folders}
                    listboxId={listboxId}
                    activeIndex={option.activeIndex}
                    offset={actions.length}
                    onActivate={option.setActiveId}
                    onSelect={onSelect}
                  />
                ) : null}
              </div>
            </div>
            <div
              className="chat-search-dialog__filters"
              role="tablist"
              aria-label="Search scope"
            >
              {FILTERS.map((item) => (
                <button
                  key={item}
                  type="button"
                  role="tab"
                  id={`${listboxId}-${item}`}
                  aria-controls={`${listboxId}-panel`}
                  aria-selected={filter === item}
                  tabIndex={filter === item ? 0 : -1}
                  onClick={() => changeFilter(item)}
                  onKeyDown={(event) => {
                    if (
                      !["ArrowLeft", "ArrowRight", "Home", "End"].includes(
                        event.key,
                      )
                    )
                      return;
                    event.preventDefault();
                    const index =
                      event.key === "Home"
                        ? 0
                        : event.key === "End"
                          ? FILTERS.length - 1
                          : (FILTERS.indexOf(item) +
                              (event.key === "ArrowRight"
                                ? 1
                                : FILTERS.length - 1)) %
                            FILTERS.length;
                    changeFilter(FILTERS[index]);
                    (
                      event.currentTarget.parentElement?.children[
                        index
                      ] as HTMLElement
                    )?.focus();
                  }}
                >
                  {item}
                </button>
              ))}
            </div>
          </form>
        </div>
      </ModalDialog>
      {creating ? (
        <CreateProjectPart
          part="CreateProjectDialog"
          onClose={() => {
            setCreating(false);
            queryRef.current?.focus();
          }}
        />
      ) : null}
    </>
  );
}
