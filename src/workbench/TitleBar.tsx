import { useCallback, useEffect, useRef, type ReactNode } from "react";
import { useWorkbenchStore } from "../app/store";
import {
  formatKeymapChords,
  keymapBinding,
  matchesPlatformChord,
} from "../domain/keymapRegistry";
import {
  CHAT_SEARCH_ALL_FOLDERS_ID,
  requestChatSearchInFolder,
} from "../features/chatSearch/chatSearchScope";
import { openQuickOpen } from "../features/quickOpen/quickOpenState";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import {
  ArrowLeft,
  ArrowRight,
  FileSearch,
  PanelLeftClose,
  PanelLeftOpen,
  Search,
  Settings,
} from "../ui/leanIcons";
import { shortcutLabel, usesMetaModifier } from "../ui/shortcutLabel";
import {
  goBackToPreviousChat,
  goForwardToNextChat,
  noteAgentChatVisit,
  useAgentChatHistory,
} from "./agentChatHistory";
import { useShownAgentChatId } from "./shownAgentChat";
import {
  WORKBENCH_WINDOW_MODES,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

const modeLabels: Record<WorkbenchWindowMode, string> = {
  agent: "Agent",
  editor: "Editor",
};

export function TitleBar({
  inert = false,
  mode,
  onModeChange,
  agentNavigationCollapsed = false,
  onToggleAgentNavigation,
  chatSlotRef,
  notices,
  terminalControls,
}: {
  inert?: boolean;
  mode: WorkbenchWindowMode;
  onModeChange: (mode: WorkbenchWindowMode) => void;
  agentNavigationCollapsed?: boolean;
  onToggleAgentNavigation?: () => void;
  /** Agent only: where the shown chat's header renders. */
  chatSlotRef?: (node: HTMLDivElement | null) => void;
  /** Agent only: what the status bar carries in Editor. */
  notices?: ReactNode;
  terminalControls?: ReactNode;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const openSettings = useWorkbenchStore((state) => state.openSettings);
  const isAgent = mode === "agent";
  const chatSearchRef = useRef<HTMLButtonElement | null>(null);
  const chatSearchDisabled = !project || projectTransitioning;
  const navigationLabel = agentNavigationCollapsed
    ? "Show projects and chats"
    : "Hide projects and chats";
  const NavigationIcon = agentNavigationCollapsed
    ? PanelLeftOpen
    : PanelLeftClose;
  const openChatSearch = useCallback(() => {
    if (!project) return;
    useAgentSettingsState.getState().close();
    requestChatSearchInFolder(CHAT_SEARCH_ALL_FOLDERS_ID, () =>
      chatSearchRef.current?.focus(),
    );
  }, [project]);
  useEffect(() => {
    if (inert || !isAgent || chatSearchDisabled) return;
    const onKeyDown = (event: KeyboardEvent) => {
      // Control+K on macOS is the text fields' kill-line, so only Command.
      if (
        !matchesPlatformChord(
          keymapBinding("chatSearch"),
          event,
          usesMetaModifier(),
        )
      )
        return;
      event.preventDefault();
      openChatSearch();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [inert, chatSearchDisabled, isAgent, openChatSearch]);
  const focusedId = useShownAgentChatId();
  const projectId = project?.id ?? null;
  useEffect(() => {
    if (!isAgent || !projectId || !focusedId || projectTransitioning) return;
    noteAgentChatVisit({ projectId, chatId: focusedId });
  }, [isAgent, projectId, focusedId, projectTransitioning]);
  const canGoBack = useAgentChatHistory((state) => state.canGoBack);
  const canGoForward = useAgentChatHistory((state) => state.canGoForward);
  const apple = usesMetaModifier();
  const chord = (id: string) => formatKeymapChords(keymapBinding(id), apple)[0];
  const backLabel = `Back (${chord("navigateBack")})`;
  const forwardLabel = `Forward (${chord("navigateForward")})`;

  const modes = (
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

  // Agent: one 40 px toolbar row. The chat's own header (title, status, more,
  // resources and the right-pane button) is portaled into `chatSlotRef`.
  if (isAgent)
    return (
      <header
        is="sage-titlebar"
        className="title-bar title-bar--agent"
        inert={inert}
        aria-hidden={inert || undefined}
      >
        <div className="title-bar__start">
          {onToggleAgentNavigation ? (
            <button
              className="title-bar__chat-search"
              type="button"
              aria-label={navigationLabel}
              title={navigationLabel}
              aria-expanded={!agentNavigationCollapsed}
              onClick={onToggleAgentNavigation}
            >
              <NavigationIcon size={16} aria-hidden="true" />
            </button>
          ) : null}
          <button
            className="title-bar__chat-search"
            type="button"
            aria-label="Back"
            title={backLabel}
            disabled={!canGoBack || projectTransitioning}
            onClick={() => void goBackToPreviousChat()}
          >
            <ArrowLeft size={16} aria-hidden="true" />
          </button>
          <button
            className="title-bar__chat-search"
            type="button"
            aria-label="Forward"
            title={forwardLabel}
            disabled={!canGoForward || projectTransitioning}
            onClick={() => void goForwardToNextChat()}
          >
            <ArrowRight size={16} aria-hidden="true" />
          </button>
          <button
            ref={chatSearchRef}
            className="title-bar__chat-search"
            type="button"
            title={`Search chats (${shortcutLabel("k", usesMetaModifier())})`}
            aria-label="Search chats"
            aria-keyshortcuts={usesMetaModifier() ? "Meta+K" : "Control+K"}
            disabled={chatSearchDisabled}
            onClick={openChatSearch}
          >
            <Search size={16} aria-hidden="true" />
          </button>
        </div>
        <div className="title-bar__chat" ref={chatSlotRef} />
        <div className="title-bar__end">
          {notices}
          {terminalControls}
          {modes}
        </div>
      </header>
    );

  return (
    <header
      is="sage-titlebar"
      className="title-bar"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <div className="title-bar__start">{modes}</div>
      <div className="title-bar__center">
        <button
          className="title-bar__search"
          type="button"
          title={`Search files (${shortcutLabel("p", usesMetaModifier())})`}
          aria-label="Search files"
          onClick={openQuickOpen}
        >
          <FileSearch size={14} aria-hidden="true" />
          <span className="title-bar__search-label">Search files</span>
        </button>
      </div>
      <div className="title-bar__end">
        <button
          className="title-bar__chat-search"
          type="button"
          title="Open settings"
          aria-label="Open settings"
          onClick={() => void openSettings()}
        >
          <Settings size={14} aria-hidden="true" />
        </button>
      </div>
    </header>
  );
}
