import { useCallback, useEffect, useRef } from "react";
import { useWorkbenchStore } from "../app/store";
import { keymapBinding, matchesPlatformChord } from "../domain/keymapRegistry";
import { requestChatSearchInFolder } from "../features/chatSearch/chatSearchScope";
import { openQuickOpen } from "../features/quickOpen/quickOpenState";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import {
  FileSearch,
  ArrowLeft,
  ArrowRight,
  PanelLeftClose,
  PanelLeftOpen,
  Search,
  Settings,
} from "../ui/leanIcons";
import { shortcutLabel, usesMetaModifier } from "../ui/shortcutLabel";
import { useAgentNavigationHistory } from "./titleBarCenter";
import {
  WORKBENCH_WINDOW_MODES,
  type WorkbenchWindowMode,
} from "./workbenchWindowMode";

const modeLabels: Record<WorkbenchWindowMode, string> = {
  agent: "Agent",
  editor: "Editor",
};

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
  agentNavigationCollapsed = false,
  onToggleAgentNavigation,
}: {
  inert?: boolean;
  mode: WorkbenchWindowMode;
  onModeChange: (mode: WorkbenchWindowMode) => void;
  agentNavigationCollapsed?: boolean;
  onToggleAgentNavigation?: () => void;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const openSettings = useWorkbenchStore((state) => state.openSettings);
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
    requestChatSearchInFolder(project.id, () => chatSearchRef.current?.focus());
  }, [project]);
  useEffect(() => {
    if (inert || mode !== "agent" || chatSearchDisabled) return;
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
  }, [inert, chatSearchDisabled, mode, openChatSearch]);
  const history = useAgentNavigationHistory(mode === "agent" && !inert);

  return (
    <header
      className="title-bar"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <div className="title-bar__start">
        <WorkbenchModeSwitch mode={mode} onModeChange={onModeChange} />
        {mode === "agent" ? (
          <>
            <button
              className="title-bar__chat-search"
              type="button"
              aria-label="Go back"
              title={history.backLabel}
              disabled={!history.canBack}
              onClick={() => void history.travel(-1)}
            >
              <ArrowLeft size={14} aria-hidden="true" />
            </button>
            <button
              className="title-bar__chat-search"
              type="button"
              aria-label="Go forward"
              title={history.forwardLabel}
              disabled={!history.canForward}
              onClick={() => void history.travel(1)}
            >
              <ArrowRight size={14} aria-hidden="true" />
            </button>
          </>
        ) : null}
        {mode === "agent" && onToggleAgentNavigation ? (
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
      </div>
      <div className="title-bar__center">
        {mode === "editor" ? (
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
