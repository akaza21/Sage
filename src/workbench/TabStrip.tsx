import {
  GitCompareArrows,
  MessageSquare,
  Plus,
  TerminalSquare,
  X,
} from "lucide-react";

import type { TabKind } from "../domain/workbench";
import { orderedTabs } from "../app/tabState";
import { useWorkbenchStore } from "../app/store";
import { FileKindIcon } from "../features/files/FileKindIcon";

const icons: Record<TabKind, typeof MessageSquare> = {
  chat: MessageSquare,
  file: MessageSquare,
  diff: GitCompareArrows,
  terminal: TerminalSquare,
  search: MessageSquare,
  problems: MessageSquare,
  output: TerminalSquare,
  settings: MessageSquare,
};

export function TabStrip({
  kinds,
  showNewChat = true,
  label = "Open tabs",
}: {
  kinds?: ReadonlyArray<TabKind>;
  showNewChat?: boolean;
  label?: string;
} = {}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const closeTab = useWorkbenchStore((state) => state.closeTab);
  const createChat = useWorkbenchStore((state) => state.createChat);
  const project = useWorkbenchStore((state) => state.project);
  const attachedChatCount = tabs.filter((tab) => tab.kind === "chat").length;
  const visibleTabs = kinds
    ? orderedTabs(tabs).filter((tab) => kinds.includes(tab.kind))
    : orderedTabs(tabs);

  return (
    <nav className="tab-strip" aria-label={label}>
      <div className="tab-strip__scroll">
        {visibleTabs.map((tab) => {
          const Icon = icons[tab.kind];
          const active = tab.id === activeTabId;

          return (
            <div
              className={`tab ${active ? "tab--active" : ""}`}
              key={tab.id}
              role="presentation"
            >
              <button
                className="tab__activate"
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => void activateTab(tab.id)}
              >
                {tab.kind === "file" ? (
                  <FileKindIcon
                    path={tab.resource?.relativePath ?? tab.title}
                    size={15}
                  />
                ) : (
                  <Icon size={15} strokeWidth={1.8} aria-hidden="true" />
                )}
                <span>{tab.title}</span>
                {tab.status === "working" ? (
                  <span className="tab__status" title="Working" />
                ) : null}
                {tab.dirty ? (
                  <span className="tab__dirty" title="Unsaved changes" />
                ) : null}
              </button>
              {!tab.pinned ? (
                <button
                  className="tab__close"
                  type="button"
                  aria-label={`${tab.kind === "chat" ? "Archive" : "Close"} ${tab.title}`}
                  disabled={tab.kind === "chat" && attachedChatCount <= 1}
                  onClick={() => closeTab(tab.id)}
                >
                  <X size={14} aria-hidden="true" />
                </button>
              ) : null}
            </div>
          );
        })}
      </div>
      {showNewChat ? (
        <button
          className="icon-button tab-strip__new"
          type="button"
          aria-label="New chat"
          title="New chat"
          disabled={!project}
          onClick={() => void createChat()}
        >
          <Plus size={17} aria-hidden="true" />
        </button>
      ) : null}
    </nav>
  );
}
