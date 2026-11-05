import {
  Activity,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { useDocumentVisible } from "../../ui/useDocumentVisible";
import {
  Archive,
  Bot,
  ChevronRight,
  Command,
  Eye,
  Globe,
  Lock,
  MessageSquare,
  Search,
  X,
} from "../../ui/leanIcons";
import { matchingSettingsEntryIds } from "../../domain/settingsNavigation";
import { loadSettingsPasswordsPage } from "../../workbench/settings/settingsPageLoaders";
import "./agent-settings.css";
import { AgentAppearanceSettings } from "./AgentAppearanceSettings";
import { AgentArchivedSettings } from "./AgentArchivedSettings";
import { AgentChatSettings } from "./AgentChatSettings";
import { AgentHarnessSettings } from "./AgentHarnessSettings";
import {
  returnFocusToAgentSettingsOpener,
  useAgentSettingsState,
  type AgentSettingsPage,
} from "./agentSettingsState";

const loadBrowserSettings = () =>
  import("./AgentBrowserSettings").then(({ AgentBrowserSettings }) => ({
    default: AgentBrowserSettings,
  }));

const loadExecution = () => import("./AgentSettingsExecution");
// The panel has no per-setting search, so the whole Passwords page shows.
const passwordEntries = matchingSettingsEntryIds("", "passwords");
const loadKeyboardSettings = () => import("./AgentKeyboardSettings");

export default AgentSettingsSurface;

// Settings order and grouping, limited to the sections Sage has.
const groups: readonly {
  heading: string | null;
  pages: readonly AgentSettingsPage[];
}[] = [
  { heading: null, pages: ["Appearance", "Keybindings"] },
  {
    heading: "Agents",
    pages: ["Harnesses", "Chats", "Browser", "Passwords"],
  },
  { heading: null, pages: ["Archived chats"] },
];

const SECTION_STEPS: Partial<Record<string, number>> = {
  ArrowDown: 1,
  ArrowUp: -1,
};

/** ArrowUp and ArrowDown move between the section buttons, wrapping. */
function moveSectionFocus(event: KeyboardEvent<HTMLElement>) {
  const step = SECTION_STEPS[event.key];
  if (!step) return;
  const sections = [
    ...event.currentTarget.querySelectorAll<HTMLButtonElement>(
      ".agent-settings__nav-group > button",
    ),
  ];
  const index = sections.indexOf(event.target as HTMLButtonElement);
  if (index < 0) return;
  event.preventDefault();
  sections[(index + step + sections.length) % sections.length]?.focus();
}

const icons: Record<AgentSettingsPage, typeof Archive> = {
  Appearance: Eye,
  Keybindings: Command,
  Harnesses: Bot,
  Chats: MessageSquare,
  Browser: Globe,
  Passwords: Lock,
  "Archived chats": Archive,
};

export function AgentSettingsSurface({
  chatRuntimeOwned = false,
}: {
  chatRuntimeOwned?: boolean;
}) {
  const documentVisible = useDocumentVisible();
  const page = useAgentSettingsState((state) => state.page) ?? "Appearance";
  const open = useAgentSettingsState((state) => state.open);
  const close = useAgentSettingsState((state) => state.close);
  const [query, setQuery] = useState("");
  const surfaceRef = useRef<HTMLElement | null>(null);
  // Settings covers the Agent workspace without unmounting it, so closing
  // it hands focus back to whatever opened it.
  useEffect(() => {
    const surface = surfaceRef.current;
    return () => returnFocusToAgentSettingsOpener(surface);
  }, []);
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
    // usable, with the settings dialog centred in it.
    <div className="agent-settings-layer">
      <section
        ref={surfaceRef}
        className="agent-settings"
        role="dialog"
        aria-modal="true"
        aria-label="Agent settings"
        onKeyDown={(event) => {
          const target = event.target as Element;
          if (
            event.key === "Escape" &&
            !event.defaultPrevented &&
            !target.closest(
              "input:not([type]):not([type=search]), input[type=text], input[type=email], input[type=password], input[type=url], input[type=tel], input[type=number], input[type=date], input[type=time], input[type=datetime-local], input[type=month], input[type=week], textarea, [contenteditable=true]",
            ) &&
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
          onKeyDown={moveSectionFocus}
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
          {chatRuntimeOwned ? null : (
            <LazyModuleBoundary
              label="Chat execution"
              load={loadExecution}
              fallback={null}
              render={(Execution) => <Execution />}
            />
          )}
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
          {/* Like the Editor workbench, pages sleep while the window is
              minimized, so they clear what they hold on suspend. */}
          <Activity mode={documentVisible ? "visible" : "hidden"}>
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
              {page === "Passwords" ? (
                <LazyModuleBoundary
                  label="Saved passwords"
                  load={loadSettingsPasswordsPage}
                  render={(SettingsPasswordsPage) => (
                    <SettingsPasswordsPage visible={passwordEntries} />
                  )}
                />
              ) : null}
              {page === "Archived chats" ? <AgentArchivedSettings /> : null}
            </div>
          </Activity>
        </div>
      </section>
    </div>
  );
}
