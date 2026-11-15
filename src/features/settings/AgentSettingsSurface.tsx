import {
  Activity,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentType,
  type KeyboardEvent,
} from "react";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import {
  hasNavigationModifier,
  listIndexForKey,
} from "../../ui/listNavigation";
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
import {
  returnFocusToAgentSettingsOpener,
  useAgentSettingsState,
  type AgentSettingsPage,
} from "./agentSettingsState";

const loadBrowserSettings = () =>
  import("./AgentBrowserSettings").then(({ AgentBrowserSettings }) => ({
    default: AgentBrowserSettings,
  }));

// The Harnesses page loads on demand, and Settings keeps the current page
// on screen until it is ready rather than flashing a loading line. It is
// fetched ahead when Settings goes idle, or when its section is pointed at.
let HarnessSettings: ComponentType | null = null;
let harnessRequest: Promise<ComponentType> | null = null;
const loadHarnessSettings = () =>
  (harnessRequest ??= import("./AgentHarnessSettings").then(
    ({ AgentHarnessSettings }) => (HarnessSettings = AgentHarnessSettings),
    (error: unknown) => {
      harnessRequest = null;
      throw error;
    },
  ));
const prewarmHarnessSettings = () =>
  void loadHarnessSettings().catch(() => undefined);
const loadHarnessModule = () =>
  loadHarnessSettings().then((Surface) => ({ default: Surface }));

const loadExecution = () => import("./AgentSettingsExecution");
// The panel has no per-setting search, so the whole Passwords page shows.
const passwordEntries = matchingSettingsEntryIds("", "passwords");
const loadKeyboardSettings = () => import("./AgentKeyboardSettings");

export default AgentSettingsSurface;

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

function moveSectionFocus(event: KeyboardEvent<HTMLElement>) {
  if (hasNavigationModifier(event)) return;
  const sections = [
    ...event.currentTarget.querySelectorAll<HTMLButtonElement>(
      ".agent-settings__nav-group > button",
    ),
  ];
  const index = sections.indexOf(event.target as HTMLButtonElement);
  if (index < 0) return;
  const next = listIndexForKey(
    event.key,
    index,
    sections.map(() => false),
    { orientation: "vertical", wrap: true },
  );
  if (next === null) return;
  event.preventDefault();
  sections[next].focus();
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
  const pageRef = useRef<HTMLDivElement | null>(null);
  const [, setHarnessReady] = useState(Boolean(HarnessSettings));
  const [harnessFailed, setHarnessFailed] = useState(false);
  const [shownPage, setShownPage] = useState(page);
  const deferredFrom = useRef<Element | null>(null);
  if (
    page !== shownPage &&
    (page !== "Harnesses" || HarnessSettings || harnessFailed)
  )
    setShownPage(page);
  useEffect(() => {
    if (page !== "Harnesses" || HarnessSettings) return;
    deferredFrom.current = document.activeElement;
    let active = true;
    loadHarnessSettings().then(
      () => active && setHarnessReady(true),
      () => active && setHarnessFailed(true),
    );
    return () => {
      active = false;
    };
  }, [page]);
  // A layout effect, so focus moves in the same commit that shows the page
  // and nothing can see the page on screen before it holds focus.
  useLayoutEffect(() => {
    if (shownPage !== "Harnesses" || !deferredFrom.current) return;
    const from = deferredFrom.current;
    deferredFrom.current = null;
    // Move into the page only if focus never left the section button that
    // asked for it, so searching or moving on while it loads is kept.
    if (document.activeElement !== from) return;
    pageRef.current
      ?.querySelector<HTMLElement>("button, input, select, textarea, a[href]")
      ?.focus();
  }, [shownPage]);
  useEffect(() => {
    const idle = window.requestIdleCallback ?? window.setTimeout;
    const cancel = window.cancelIdleCallback ?? window.clearTimeout;
    const handle = idle(prewarmHarnessSettings);
    return () => cancel(handle);
  }, []);
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
                    {...(item === "Harnesses"
                      ? {
                          onPointerEnter: prewarmHarnessSettings,
                          onFocus: prewarmHarnessSettings,
                        }
                      : {})}
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
            <h2>{shownPage}</h2>
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
            <div className="agent-settings__page" key={shownPage} ref={pageRef}>
              {shownPage === "Appearance" ? <AgentAppearanceSettings /> : null}
              {shownPage === "Keybindings" ? (
                <LazyModuleBoundary
                  label="Keybindings"
                  load={loadKeyboardSettings}
                  render={(KeyboardSettings) => <KeyboardSettings />}
                />
              ) : null}
              {shownPage === "Harnesses" ? (
                HarnessSettings ? (
                  <HarnessSettings />
                ) : (
                  <LazyModuleBoundary
                    label="Harness settings"
                    load={loadHarnessModule}
                    render={(Surface) => <Surface />}
                  />
                )
              ) : null}
              {shownPage === "Chats" ? <AgentChatSettings /> : null}
              {shownPage === "Browser" ? (
                <LazyModuleBoundary
                  label="Browser settings"
                  load={loadBrowserSettings}
                  render={(AgentBrowserSettings) => <AgentBrowserSettings />}
                />
              ) : null}
              {shownPage === "Passwords" ? (
                <LazyModuleBoundary
                  label="Saved passwords"
                  load={loadSettingsPasswordsPage}
                  render={(SettingsPasswordsPage) => (
                    <SettingsPasswordsPage visible={passwordEntries} />
                  )}
                />
              ) : null}
              {shownPage === "Archived chats" ? (
                <AgentArchivedSettings />
              ) : null}
            </div>
          </Activity>
        </div>
      </section>
    </div>
  );
}
