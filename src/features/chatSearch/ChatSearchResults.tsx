import { chatDisplayTitle } from "../../domain/chatTitle";
import {
  chatSearchResultFolderName,
  isChatSearchFanout,
  type ChatSearchFanoutPick,
  type ChatSearchFanoutView,
} from "./chatSearchFanout";
import type {
  ChatSearchFolderOption,
  ChatSearchScope,
} from "./chatSearchScope";

export function ChatSearchResults({
  response,
  error,
  scope,
  folders,
  onFocusQuery,
  onSelect,
}: {
  response: ChatSearchFanoutView | null;
  error: string | null;
  scope: ChatSearchScope;
  folders: readonly ChatSearchFolderOption[];
  onFocusQuery: () => void;
  onSelect: (result: ChatSearchFanoutPick) => Promise<void>;
}) {
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
            <div
              role="listbox"
              aria-label="Chat search results"
              onKeyDown={(event) => {
                if (event.key !== "ArrowDown" && event.key !== "ArrowUp")
                  return;
                event.preventDefault();
                const options = [
                  ...event.currentTarget.querySelectorAll<HTMLButtonElement>(
                    '[role="option"]',
                  ),
                ];
                const index = options.indexOf(
                  document.activeElement as HTMLButtonElement,
                );
                if (event.key === "ArrowUp" && index <= 0) onFocusQuery();
                else
                  options[
                    Math.min(
                      options.length - 1,
                      index + (event.key === "ArrowDown" ? 1 : -1),
                    )
                  ]?.focus();
              }}
            >
              {response.results.map((result) => {
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
                return (
                  <button
                    className="chat-search-result"
                    key={`${result.projectId}:${result.chatId}`}
                    type="button"
                    role="option"
                    aria-selected="false"
                    aria-label={[title, ...details].join(" — ")}
                    title={title}
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
        </>
      ) : null}
    </div>
  );
}
