import { Plus, X } from "lucide-react";
import { useRef, type KeyboardEvent } from "react";

import { chatDisplayTitle } from "../../domain/chatTitle";
import type { WorkbenchTab } from "../../domain/workbench";

export function ChatSessionStrip({
  tabs,
  selectedId,
  onSelect,
  onClose,
  onCreate,
  disabled = false,
}: {
  tabs: readonly WorkbenchTab[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onCreate: () => void;
  disabled?: boolean;
}) {
  const tabRefs = useRef(new Map<string, HTMLButtonElement>());

  function moveFocus(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    if (disabled || tabs.length === 0) return;
    let nextIndex: number | null = null;
    if (event.key === "ArrowRight") nextIndex = (index + 1) % tabs.length;
    if (event.key === "ArrowLeft")
      nextIndex = (index - 1 + tabs.length) % tabs.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = tabs.length - 1;
    if (nextIndex === null) return;
    event.preventDefault();
    const next = tabs[nextIndex];
    if (!next) return;
    tabRefs.current.get(next.id)?.focus();
  }

  return (
    <div
      className="agent-chat-tabs"
      role="tablist"
      aria-label="Chat tabs"
      aria-orientation="horizontal"
    >
      <div className="agent-chat-tabs__scroll">
        {tabs.map((tab, index) => {
          const title = chatDisplayTitle(tab.title);
          const selected = tab.id === selectedId;
          return (
            <div
              className={`agent-chat-tab${selected ? " agent-chat-tab--active" : ""}`}
              key={tab.id}
            >
              <button
                ref={(node) => {
                  if (node) tabRefs.current.set(tab.id, node);
                  else tabRefs.current.delete(tab.id);
                }}
                className="agent-chat-tab__select"
                type="button"
                role="tab"
                aria-selected={selected}
                aria-controls="agent-chat-panel"
                disabled={disabled}
                tabIndex={selected ? 0 : -1}
                onClick={() => onSelect(tab.id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    onSelect(tab.id);
                    return;
                  }
                  moveFocus(event, index);
                }}
              >
                <span className="agent-chat-tab__title">{title}</span>
              </button>
              <button
                className="agent-chat-tab__close"
                type="button"
                aria-label={`Archive chat ${title}`}
                title={`Archive chat ${title}`}
                disabled={disabled || tabs.length <= 1}
                onClick={(event) => {
                  event.stopPropagation();
                  onClose(tab.id);
                }}
              >
                <X size={14} aria-hidden="true" />
              </button>
            </div>
          );
        })}
      </div>
      <button
        className="agent-chat-tabs__new"
        type="button"
        aria-label="New agent chat"
        title="New agent chat"
        disabled={disabled}
        onClick={onCreate}
      >
        <Plus size={16} aria-hidden="true" />
      </button>
    </div>
  );
}
