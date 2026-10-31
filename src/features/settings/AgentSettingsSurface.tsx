import { useEffect, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { browserLiveTabs } from "../../ports/nativeBrowserPort";
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
import "./agent-settings.css";
import { AgentAppearanceSettings } from "./AgentAppearanceSettings";
import { AgentArchivedSettings } from "./AgentArchivedSettings";
import { AgentHarnessSettings } from "./AgentHarnessSettings";
import {
  useAgentSettingsState,
  type AgentSettingsPage,
} from "./agentSettingsState";

const loadBrowserSettings = () =>
  import("./AgentBrowserSettings").then(({ AgentBrowserSettings }) => ({
    default: AgentBrowserSettings,
  }));

const loadExecution = () => import("./AgentSettingsExecution");
const loadKeyboardSettings = () => import("./AgentKeyboardSettings");
const loadChatSettings = () =>
  import("./AgentChatSettings").then(({ AgentChatSettings }) => ({
    default: AgentChatSettings,
  }));

export default AgentSettingsSurface;

// Settings order and grouping, limited to the sections Sage has.
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

const CONTROL_FOCUS_TIMEOUT_MS = 5_000;
const searchControls: readonly {
  page: AgentSettingsPage;
  label: string;
  aliases: string;
  selector?: string;
  capability?: "browser" | "access";
}[] = [
  {
    page: "Appearance",
    label: "Interface font size",
    aliases: "text",
  },
  {
    page: "Appearance",
    label: "Interface density",
    aliases: "compact comfortable spacing",
  },
  {
    page: "Chats",
    label: "Default access",
    aliases: "permissions approval",
    capability: "access",
  },
  {
    page: "Chats",
    label: "Current harness",
    aliases: "agent",
  },
  {
    page: "Chats",
    label: "Current model",
    aliases: "model",
  },
  {
    page: "Browser",
    label: "Browsing history",
    aliases: "visited pages",
    selector: "#settings-history",
    capability: "browser",
  },
];

export function AgentSettingsSurface() {
  const page = useAgentSettingsState((state) => state.page) ?? "Appearance";
  const open = useAgentSettingsState((state) => state.open);
  const close = useAgentSettingsState((state) => state.close);
  const [query, setQuery] = useState("");
  const [focusRequest, setFocusRequest] = useState<{ selector: string } | null>(
    null,
  );
  const [historyAvailable, setHistoryAvailable] = useState(false);
  useEffect(() => {
    if (!isTauriRuntime() || !query.trim()) return;
    let current = true;
    void browserLiveTabs().then(
      (ids) => {
        if (current) setHistoryAvailable(ids.length > 0);
      },
      () => {
        if (current) setHistoryAvailable(false);
      },
    );
    return () => {
      current = false;
    };
  }, [query]);
  const accessAvailable = useWorkbenchStore((state) =>
    state.harnesses.some(
      (harness) =>
        harness.id === state.selectedHarnessId && harness.accessModes?.length,
    ),
  );
  useEffect(() => {
    if (!focusRequest) return;
    const focus = () => {
      const control = document.querySelector<HTMLElement>(
        `.agent-settings__page ${focusRequest.selector}`,
      );
      if (!control || control.matches(":disabled")) return false;
      control.focus();
      control.scrollIntoView?.({ block: "nearest" });
      return true;
    };
    if (focus()) return;
    const observer = new MutationObserver(() => {
      if (focus()) observer.disconnect();
    });
    observer.observe(document.querySelector(".agent-settings__page")!, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["disabled"],
    });
    const timer = window.setTimeout(
      () => observer.disconnect(),
      CONTROL_FOCUS_TIMEOUT_MS,
    );
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, [focusRequest, page]);
  useEffect(() => {
    const launcher = useAgentSettingsState.getState().launcher;
    return () => {
      // Activity resumes child effects before resource reconciliation settles.
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (useAgentSettingsState.getState().page) return;
          const target = [
            launcher,
            ...[
              ".agent-chat-pane--focused textarea",
              ".agent-chat-pane--focused",
              ".title-bar input:checked",
            ].map((selector) => document.querySelector<HTMLElement>(selector)),
          ].find(
            (element) =>
              element?.isConnected &&
              element.getClientRects().length &&
              !element.matches(":disabled") &&
              !element.closest("[inert]"),
          );
          target?.focus();
        }),
      );
    };
  }, []);
  const normalized = query.trim().toLowerCase();
  const results = normalized
    ? searchControls.filter((control) =>
        `${control.label} ${control.aliases}`
          .toLowerCase()
          .includes(normalized),
      )
    : [];
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
        className="agent-settings"
        role="region"
        aria-label="Agent settings"
        onKeyDown={(event) => {
          const target = event.target as Element;
          if (
            event.key === "Escape" &&
            !event.defaultPrevented &&
            !target.closest(
              "input:not([type]):not([type=search]), input[type=text], input[type=email], input[type=password], input[type=url], input[type=tel], input[type=number], input[type=date], input[type=time], input[type=datetime-local], input[type=month], input[type=week], textarea, [contenteditable=true]",
            ) &&
            !target.closest(
              "dialog, [role=dialog], [role=menu], [role=listbox]",
            ) &&
            !(
              target instanceof HTMLInputElement &&
              target.type === "search" &&
              target.value
            )
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
              maxLength={256}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          {results.map((control) => (
            <button
              className="button button--compact"
              type="button"
              key={control.label}
              disabled={
                (control.capability === "browser" && !historyAvailable) ||
                (control.capability === "access" && !accessAvailable)
              }
              onClick={() => {
                open(control.page);
                setFocusRequest({
                  selector:
                    control.selector ?? `[aria-label="${control.label}"]`,
                });
              }}
            >
              {control.page}: {control.label}
            </button>
          ))}
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
          {!matching.length && !results.length ? (
            <p role="status">No matching sections.</p>
          ) : null}
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
            {page === "Chats" ? (
              <LazyModuleBoundary
                label="Chat settings"
                load={loadChatSettings}
                render={(ChatSettings) => <ChatSettings />}
              />
            ) : null}
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
