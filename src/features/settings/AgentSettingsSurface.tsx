import { useState } from "react";
import {
  useAgentSettingsState,
  type AgentSettingsPage,
} from "./agentSettingsState";
import "./agent-settings.css";
import { AgentChatSettings } from "./AgentChatSettings";
import { AgentHarnessSettings } from "./AgentHarnessSettings";
import { AgentArchivedSettings } from "./AgentArchivedSettings";
import { AgentAppearanceSettings } from "./AgentAppearanceSettings";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import {
  Archive,
  Bot,
  ChevronRight,
  Command,
  Eye,
  Globe,
  MessageSquare,
  Search,
  X,
} from "../../ui/leanIcons";

const loadBrowserSettings = () =>
  import("./AgentBrowserSettings").then(({ AgentBrowserSettings }) => ({
    default: AgentBrowserSettings,
  }));

const loadExecution = () => import("./AgentSettingsExecution");
const loadKeyboardSettings = () => import("./AgentKeyboardSettings");

export default AgentSettingsSurface;

// T3 Code's settings order and grouping, limited to the sections Sage has.
const groups: readonly {
  heading: string | null;
  pages: readonly AgentSettingsPage[];
}[] = [
  { heading: null, pages: ["Appearance", "Keybindings"] },
  { heading: "Agents", pages: ["Harnesses", "Chats", "Browser"] },
  { heading: null, pages: ["Archived chats"] },
];

const icons: Record<AgentSettingsPage, typeof Archive> = {
  Appearance: Eye,
  Keybindings: Command,
  Harnesses: Bot,
  Chats: MessageSquare,
  Browser: Globe,
  "Archived chats": Archive,
};

export function AgentSettingsSurface() {
  const page = useAgentSettingsState((state) => state.page) ?? "Appearance";
  const open = useAgentSettingsState((state) => state.open);
  const close = useAgentSettingsState((state) => state.close);
  const [query, setQuery] = useState("");
  const normalized = query.trim().toLowerCase();
  const matching = groups
    .map((group) => ({
      ...group,
      pages: group.pages.filter((item) =>
        item.toLowerCase().includes(normalized),
      ),
    }))
    .filter((group) => group.pages.length > 0);
  return (
    // Takes the Agent workspace's place below the title bar, which stays
    // usable, with T3 Code's settings dialog centred in it.
    <div className="agent-settings-layer">
      <section
        className="agent-settings"
        role="dialog"
        aria-modal="true"
        aria-label="Agent settings"
        onKeyDown={(event) => {
          const target = event.target as Element;
          if (
            event.key === "Escape" &&
            !event.defaultPrevented &&
            !target.closest("input:not([type=search]), select, textarea") &&
            target.closest("[role=dialog]") === event.currentTarget &&
            !(target.matches("input[type=search]") && query)
          ) {
            event.preventDefault();
            close();
          }
        }}
      >
        <nav
          className="agent-settings__nav"
          aria-label="Agent settings sections"
        >
          <label className="input agent-settings__search">
            <Search size={14} aria-hidden="true" />
            <span className="sr-only">Search Agent settings sections</span>
            <input
              autoFocus
              type="search"
              placeholder="Search settings"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          {matching.map((group) => (
            <div className="agent-settings__nav-group" key={group.pages[0]}>
              {group.heading ? <h2>{group.heading}</h2> : null}
              {group.pages.map((item) => {
                const Icon = icons[item];
                return (
                  <button
                    key={item}
                    type="button"
                    aria-current={page === item ? "page" : undefined}
                    onClick={() => open(item)}
                  >
                    <Icon size={16} aria-hidden="true" />
                    {item}
                  </button>
                );
              })}
            </div>
          ))}
          {!matching.length ? <p role="status">No matching sections.</p> : null}
        </nav>
        <div className="agent-settings__content">
          <LazyModuleBoundary
            label="Chat execution"
            load={loadExecution}
            fallback={null}
            render={(Execution) => <Execution />}
          />
          <header className="agent-settings__header">
            <span>Settings</span>
            <ChevronRight size={14} aria-hidden="true" />
            <h2>{page}</h2>
            <button
              type="button"
              className="icon-button"
              aria-label="Close settings"
              onClick={close}
            >
              <X size={16} aria-hidden="true" />
            </button>
          </header>
          <div className="agent-settings__page" key={page}>
            {page === "Appearance" ? <AgentAppearanceSettings /> : null}
            {page === "Keybindings" ? (
              <LazyModuleBoundary
                label="Keybindings"
                load={loadKeyboardSettings}
                render={(KeyboardSettings) => <KeyboardSettings />}
              />
            ) : null}
            {page === "Harnesses" ? <AgentHarnessSettings /> : null}
            {page === "Chats" ? <AgentChatSettings /> : null}
            {page === "Browser" ? (
              <LazyModuleBoundary
                label="Browser settings"
                load={loadBrowserSettings}
                render={(AgentBrowserSettings) => <AgentBrowserSettings />}
              />
            ) : null}
            {page === "Archived chats" ? <AgentArchivedSettings /> : null}
          </div>
        </div>
      </section>
    </div>
  );
}
