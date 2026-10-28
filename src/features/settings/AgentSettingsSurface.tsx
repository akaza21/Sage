import { useState } from "react";
import {
  useAgentSettingsState,
  type AgentSettingsPage,
} from "./agentSettingsState";
import "./agent-settings.css";
import { AgentGeneralSettings } from "./AgentGeneralSettings";
import { AgentHarnessSettings } from "./AgentHarnessSettings";
import { AgentArchivedSettings } from "./AgentArchivedSettings";
import { AgentAppearanceSettings } from "./AgentAppearanceSettings";
import { SettingsRow } from "./SettingsRow";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";

const loadBrowserSettings = () =>
  import("./AgentBrowserSettings").then(({ AgentBrowserSettings }) => ({
    default: AgentBrowserSettings,
  }));

const loadExecution = () => import("./AgentSettingsExecution");
const loadKeyboardSettings = () => import("./AgentKeyboardSettings");

export default AgentSettingsSurface;

const pages: readonly AgentSettingsPage[] = [
  "General",
  "Harnesses",
  "Browser",
  "Keyboard shortcuts",
  "Archived chats",
  "Appearance",
  "Notifications",
  "Approvals",
];

export function AgentSettingsSurface() {
  const page = useAgentSettingsState((state) => state.page) ?? "General";
  const open = useAgentSettingsState((state) => state.open);
  const close = useAgentSettingsState((state) => state.close);
  const [query, setQuery] = useState("");
  const matching = pages.filter((item) =>
    item.toLowerCase().includes(query.trim().toLowerCase()),
  );
  return (
    <section
      className="agent-settings"
      aria-label="Agent settings"
      onKeyDown={(event) => {
        if (
          event.key === "Escape" &&
          !event.defaultPrevented &&
          !(event.target as Element).closest(
            "input, select, textarea, [role=dialog]",
          )
        ) {
          event.preventDefault();
          close();
        }
      }}
    >
      <nav className="agent-settings__nav" aria-label="Agent settings sections">
        <button
          type="button"
          className="button button--compact"
          onClick={close}
        >
          Back to chats
        </button>
        <h1>Settings</h1>
        <label>
          <span className="sr-only">Search Agent settings sections</span>
          <input
            className="input"
            type="search"
            placeholder="Search settings"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <p>Agent</p>
        {matching.map((item) => (
          <button
            key={item}
            type="button"
            aria-current={page === item ? "page" : undefined}
            onClick={() => open(item)}
          >
            {item}
          </button>
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
        <div className="agent-settings__page" key={page}>
          <h2>{page}</h2>
          {page === "General" ? <AgentGeneralSettings /> : null}
          {page === "Browser" ? (
            <LazyModuleBoundary
              label="Browser settings"
              load={loadBrowserSettings}
              render={(AgentBrowserSettings) => <AgentBrowserSettings />}
            />
          ) : null}
          {page === "Harnesses" ? <AgentHarnessSettings /> : null}
          {page === "Archived chats" ? <AgentArchivedSettings /> : null}
          {page === "Appearance" ? <AgentAppearanceSettings /> : null}
          {page === "Keyboard shortcuts" ? (
            <LazyModuleBoundary
              label="Keyboard shortcuts"
              load={loadKeyboardSettings}
              render={(KeyboardSettings) => <KeyboardSettings />}
            />
          ) : null}
          {page === "Approvals" ? (
            <div className="agent-settings__card">
              <SettingsRow
                title="Remembered approvals"
                description="Sage does not expose a reset command for harness-managed 'Don't ask again' decisions. Manage these in your harness; access defaults do not revoke remembered approvals."
              >
                <span>Unavailable</span>
              </SettingsRow>
            </div>
          ) : null}
          {page === "Notifications" ? (
            <div className="agent-settings__card">
              <SettingsRow
                title="Desktop notifications"
                description="Sage does not expose notification preferences yet."
              >
                <span>Unavailable</span>
              </SettingsRow>
            </div>
          ) : null}
        </div>
      </div>
    </section>
  );
}
