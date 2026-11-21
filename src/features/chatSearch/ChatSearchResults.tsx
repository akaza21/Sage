import { chatDisplayTitle } from "../../domain/chatTitle";
import {
  chatSearchResultFolderName,
  chatSearchResultKey,
  type ChatSearchFanoutPick,
  type ChatSearchFanoutView,
} from "./chatSearchFanout";
import type { ChatSearchFolderOption } from "./chatSearchScope";

export function ChatSearchResults({
  response,
  error,
  folders,
  listboxId,
  activeIndex,
  offset,
  onActivate,
  onSelect,
}: {
  response: ChatSearchFanoutView | null;
  error: string | null;
  folders: readonly ChatSearchFolderOption[];
  listboxId: string;
  activeIndex: number;
  offset: number;
  onActivate: (id: string) => void;
  onSelect: (result: ChatSearchFanoutPick) => Promise<void>;
}) {
  return (
    <div
      className="chat-search-dialog__results"
      role="group"
      aria-label="Chats"
    >
      <h3 className="chat-search-dialog__heading">Chats</h3>
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
            <div>
              {response.results.map((result, index) => {
                const folderName = chatSearchResultFolderName(
                  folders,
                  result.projectId,
                );
                const title = chatDisplayTitle(result.title);
                const details = [
                  folderName ?? "No project",
                  result.attached ? null : "Archived",
                ].filter((detail): detail is string => Boolean(detail));
                const key = chatSearchResultKey(result);
                return (
                  <div
                    className="chat-search-result"
                    key={key}
                    id={`${listboxId}-${index + offset}`}
                    role="option"
                    aria-selected={index + offset === activeIndex}
                    aria-label={[title, ...details].join(" — ")}
                    title={title}
                    // The query field keeps the focus, and so the keys.
                    onMouseDown={(event) => event.preventDefault()}
                    onMouseMove={() => onActivate(key)}
                    onClick={() => void onSelect(result)}
                  >
                    <span
                      className="chat-search-result__dot"
                      aria-hidden="true"
                    />
                    <span className="chat-search-result__title">{title}</span>
                    <span className="chat-search-result__details">
                      {details.join(" · ")}
                    </span>
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
