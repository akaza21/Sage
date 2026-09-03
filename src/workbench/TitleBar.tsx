import { FileSearch, Search, Settings } from "lucide-react";
import { useRef } from "react";

import { useWorkbenchStore } from "../app/store";
import { requestChatSearchInFolder } from "../features/chatSearch/chatSearchScope";
import { titleBarFocusedChatId, titleBarSessionTitle } from "./titleBarCenter";
import {
  collectChatTabIds,
  WORKBENCH_WINDOW_MODES,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

const modeLabels: Record<WorkbenchWindowMode, string> = {
  agent: "Agent",
  editor: "Editor",
};

export function TitleBar({
  mode,
  onModeChange,
}: {
  mode: WorkbenchWindowMode;
  onModeChange: (mode: WorkbenchWindowMode) => void;
}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const project = useWorkbenchStore((state) => state.project);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const setSidebarView = useWorkbenchStore((state) => state.setSidebarView);
  const openSettings = useWorkbenchStore((state) => state.openSettings);
  const chatSearchRef = useRef<HTMLButtonElement | null>(null);
  const focusedId = titleBarFocusedChatId(collectChatTabIds(tabs), activeTabId);
  const sessionTitle =
    mode === "agent"
      ? titleBarSessionTitle(
          tabs.find((tab) => tab.id === focusedId)?.title ?? null,
        )
      : null;

  return (
    <header className="title-bar">
      <div className="title-bar__start">
        <div className="title-bar__brand">
          <span className="title-bar__logo">S</span>
          <strong>Sage</strong>
        </div>
        <div
          className="title-bar__modes"
          role="radiogroup"
          aria-label="Window mode"
        >
          {WORKBENCH_WINDOW_MODES.map((value) => (
            <label
              key={value}
              className={`title-bar__mode${
                mode === value ? " title-bar__mode--active" : ""
              }`}
            >
              <input
                checked={mode === value}
                name="workbench-window-mode"
                onChange={() => onModeChange(value)}
                type="radio"
                value={value}
              />
              {modeLabels[value]}
            </label>
          ))}
        </div>
      </div>
      <div className="title-bar__center">
        {mode === "editor" ? (
          <button
            className="title-bar__search"
            type="button"
            onClick={() => setSidebarView("search")}
          >
            <FileSearch size={14} aria-hidden="true" />
            Search files
          </button>
        ) : sessionTitle ? (
          <h1 className="title-bar__session">{sessionTitle}</h1>
        ) : null}
      </div>
      <div className="title-bar__end">
        {mode === "agent" ? (
          <button
            ref={chatSearchRef}
            className="title-bar__chat-search"
            type="button"
            title="Search chats"
            aria-label="Search chats"
            disabled={!project || projectTransitioning}
            onClick={() => {
              if (project) {
                requestChatSearchInFolder(project.id, () =>
                  chatSearchRef.current?.focus(),
                );
              }
            }}
          >
            <Search size={14} aria-hidden="true" />
          </button>
        ) : (
          <button
            className="title-bar__chat-search"
            type="button"
            title="Open settings"
            aria-label="Open settings"
            onClick={() => void openSettings()}
          >
            <Settings size={14} aria-hidden="true" />
          </button>
        )}
      </div>
    </header>
  );
}
