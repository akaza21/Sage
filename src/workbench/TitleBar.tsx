import { FileSearch, Search, Settings } from "../ui/leanIcons";
import { useCallback, useEffect, useRef } from "react";

import { useWorkbenchStore } from "../app/store";
import { keymapBinding, matchesKeymapChord } from "../domain/keymapRegistry";
import { shortcutLabel, usesMetaModifier } from "../ui/shortcutLabel";
import { requestChatSearchInFolder } from "../features/chatSearch/chatSearchScope";
import { titleBarFocusedChatId, titleBarSessionTitle } from "./titleBarCenter";
import { useRailChatId } from "./sideChatState";
import {
  collectChatTabIds,
  WORKBENCH_WINDOW_MODES,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

const modeLabels: Record<WorkbenchWindowMode, string> = {
  agent: "Agent",
  editor: "Editor",
};

function WorkbenchBrand() {
  return (
    <div className="title-bar__brand">
      <svg
        className="title-bar__mark"
        viewBox="0 0 24 24"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth="1"
        strokeLinejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        <path
          transform="rotate(-17 12 12)"
          d="M6.03 12.16 A 17 17 0 0 1 18.35 3.22 A 17 17 0 0 1 16.67 8.28 Z"
        />
        <path
          transform="rotate(-17 12 12)"
          d="M17.97 11.85 A 17 17 0 0 1 5.65 20.79 A 17 17 0 0 1 7.33 15.73 Z"
        />
      </svg>
      <strong className="title-bar__wordmark">Sage</strong>
    </div>
  );
}

function WorkbenchModeSwitch({
  mode,
  onModeChange,
}: {
  mode: WorkbenchWindowMode;
  onModeChange: (mode: WorkbenchWindowMode) => void;
}) {
  return (
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
  );
}

export function TitleBar({
  inert = false,
  mode,
  onModeChange,
}: {
  inert?: boolean;
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
  const chatSearchDisabled = !project || projectTransitioning;
  const openChatSearch = useCallback(() => {
    if (!project) return;
    requestChatSearchInFolder(project.id, () => chatSearchRef.current?.focus());
  }, [project]);
  useEffect(() => {
    if (inert || mode !== "agent" || chatSearchDisabled) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (!matchesKeymapChord(keymapBinding("chatSearch"), event)) return;
      event.preventDefault();
      openChatSearch();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [inert, chatSearchDisabled, mode, openChatSearch]);
  // A side chat keeps its parent's window title (Codex s2/f0244).
  const focusedId = useRailChatId(
    titleBarFocusedChatId(collectChatTabIds(tabs), activeTabId),
  );
  const sessionTitle =
    mode === "agent"
      ? titleBarSessionTitle(
          tabs.find((tab) => tab.id === focusedId)?.title ?? null,
        )
      : null;

  return (
    <header
      className="title-bar"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <div className="title-bar__start">
        <WorkbenchBrand />
        <WorkbenchModeSwitch mode={mode} onModeChange={onModeChange} />
      </div>
      <div className="title-bar__center">
        {mode === "editor" ? (
          <button
            className="title-bar__search"
            type="button"
            title="Search files"
            aria-label="Search files"
            onClick={() => setSidebarView("search")}
          >
            <FileSearch size={14} aria-hidden="true" />
            <span className="title-bar__search-label">Search files</span>
          </button>
        ) : sessionTitle ? (
          <h1 className="title-bar__session">{sessionTitle}</h1>
        ) : null}
      </div>
      <div className="title-bar__end">
        {mode === "agent" ? (
          <button
            ref={chatSearchRef}
            className="title-bar__chat-search title-bar__chat-search--pill"
            type="button"
            title="Search chats"
            aria-label="Search chats"
            aria-keyshortcuts={usesMetaModifier() ? "Meta+K" : "Control+K"}
            disabled={chatSearchDisabled}
            onClick={openChatSearch}
          >
            <Search size={14} aria-hidden="true" />
            <span className="title-bar__chat-search-label">Search chats</span>
            <kbd className="title-bar__chord">
              {shortcutLabel("k", usesMetaModifier())}
            </kbd>
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
