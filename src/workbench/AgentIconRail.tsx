import { useContext } from "react";
import { useWorkbenchStore } from "../app/store";
import { harnessHasSignInCheck } from "../domain/harness";
import { ComposerOptionsPart } from "../features/chat/lazyParts";
import {
  Archive,
  Bot,
  Box,
  FolderOpen,
  Globe,
  History,
  Home,
  Settings,
} from "../ui/leanIcons";
import { useAgentPanelView, type AgentPanelView } from "./agentPanelView";
import { AgentHomeNavigationContext } from "./agentRailNavigation";
import { expandAgentRailSection } from "./agentRailViewPrefs";

const RAIL_ITEMS: ReadonlyArray<
  [view: AgentPanelView, label: string, Icon: typeof Home]
> = [
  ["home", "Home", Home],
  ["recent", "Recent chats", History],
  ["projects", "Projects", FolderOpen],
  ["archived", "Archived chats", Archive],
  ["harnesses", "Harnesses", Box],
];

/**
 * The 48 px Agent icon rail. Each top item switches what the panel beside it
 * lists; the bottom holds the harness sign-in state, Remote and Settings.
 * `onShow` lets the collapsed rail bring the panel back first.
 */
export function AgentIconRail({
  inert = false,
  onSettings,
  onShow,
}: {
  inert?: boolean;
  onSettings: () => void;
  onShow?: (view: AgentPanelView) => void;
}) {
  const navigateHome = useContext(AgentHomeNavigationContext);
  const view = useAgentPanelView((state) => state.view);
  const signInChecks = useWorkbenchStore((state) =>
    state.harnesses.some(harnessHasSignInCheck),
  );
  const show = (next: AgentPanelView) => {
    if (next === "projects") expandAgentRailSection("projects");
    useAgentPanelView.getState().show(next);
    onShow?.(next);
    if (next === "home") navigateHome();
  };
  const harnessIcon = <Bot size={18} aria-hidden="true" />;
  return (
    <nav className="project-sidebar__icon-rail" aria-label="Sidebar sections">
      {RAIL_ITEMS.map(([value, label, Icon]) => (
        <button
          key={value}
          type="button"
          aria-label={label}
          aria-pressed={view === value}
          title={label}
          onClick={() => show(value)}
        >
          <Icon size={18} aria-hidden="true" />
        </button>
      ))}
      <span className="project-sidebar__rail-gap" />
      {signInChecks && !inert ? (
        <ComposerOptionsPart
          part="HarnessAccountMenu"
          avatar={harnessIcon}
          onShowHarnesses={() => show("harnesses")}
          fallback={<span className="account-avatar" aria-hidden="true" />}
        />
      ) : (
        <button
          type="button"
          aria-label="Harnesses: sign-in status"
          title="Harnesses"
          onClick={() => show("harnesses")}
        >
          {harnessIcon}
        </button>
      )}
      <button
        type="button"
        aria-label="Remote: not set up yet"
        title="Remote: not set up yet"
        disabled
      >
        <Globe size={18} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Open settings"
        title="Settings"
        onClick={onSettings}
      >
        <Settings size={18} aria-hidden="true" />
      </button>
    </nav>
  );
}
