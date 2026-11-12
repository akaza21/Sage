import { useLayoutEffect } from "react";

import { chatDisplayTitle } from "../../domain/chatTitle";
import {
  chatSearchResultFolderName,
  chatSearchResultKey,
  isChatSearchFanout,
  type ChatSearchFanoutPick,
  type ChatSearchFanoutView,
} from "./chatSearchFanout";
import type {
  ChatSearchFolderOption,
  ChatSearchScope,
} from "./chatSearchScope";

/**
 * The results listbox of the Search chats combobox. The query field keeps
 * the focus and names the active row with aria-activedescendant; rows take
 * pointer picks without taking the focus.
 */
export function ChatSearchResults({
  response,
  error,
  scope,
  folders,
  listboxId,
  activeIndex,
  onSelect,
}: {
  response: ChatSearchFanoutView | null;
  error: string | null;
  scope: ChatSearchScope;
  folders: readonly ChatSearchFolderOption[];
  listboxId: string;
  activeIndex: number;
  onSelect: (result: ChatSearchFanoutPick) => Promise<void>;
}) {
  useLayoutEffect(() => {
    if (activeIndex < 0) return;
    document
      .getElementById(`${listboxId}-${activeIndex}`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [activeIndex, listboxId]);
  return (
    <div className="chat-search-dialog__results">
      {error ? (
        <div className="composer__error" role="alert">
          {error}
        </div>
      ) : null}
      {response ? (
        <>
          {response.results.length === 0 ? (
            <p className="chat-search-dialog__empty">
              No chats matched “{response.query}”.
            </p>
          ) : (
            <div id={listboxId} role="listbox" aria-label="Chat search results">
              {response.results.map((result, index) => {
                const folderName = chatSearchResultFolderName(
                  folders,
                  result.projectId,
                );
                const showFolder =
                  Boolean(folderName) &&
                  (isChatSearchFanout(scope.searchProjectId) ||
                    result.projectId !== scope.activeProjectId);
                const title = chatDisplayTitle(result.title);
                // Attached is the usual case; only Archived and another
                // folder tell one row from the next.
                const details = [
                  showFolder ? folderName : null,
                  result.attached ? null : "Archived",
                ].filter((detail): detail is string => Boolean(detail));
                const key = chatSearchResultKey(result);
                return (
                  <div
                    className="chat-search-result"
                    key={key}
                    id={`${listboxId}-${index}`}
                    role="option"
                    aria-selected={index === activeIndex}
                    aria-label={[title, ...details].join(" — ")}
                    title={title}
                    // The query field keeps the focus, and so the keys.
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => void onSelect(result)}
                  >
                    <span className="chat-search-result__heading">
                      <span className="chat-search-result__title">{title}</span>
                      {details.length > 0 ? (
                        <span className="chat-search-result__details">
                          {details.join(" · ")}
                        </span>
                      ) : null}
                    </span>
                    {result.snippet ? (
                      <span className="chat-search-result__snippet">
                        {result.snippet}
                      </span>
                    ) : null}
                  </div>
                );
              })}
            </div>
          )}
          {response.hasMore ? (
            <p className="chat-search-dialog__more">
              More matches exist. Refine the query to narrow the results.
            </p>
          ) : null}
        </>
      ) : null}
    </div>
  );
}
