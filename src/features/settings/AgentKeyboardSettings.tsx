import type { KeymapScope } from "../../domain/keymapRegistry";
import { SETTINGS_KEYMAP_BINDINGS } from "../../domain/settingsNavigation";
import { SettingsKeyboardPage } from "../../workbench/settings/SettingsKeyboardPage";
import {
  BROWSER_SHORTCUT_BINDINGS,
  type BrowserShortcut,
} from "../browser/browserShortcutRegistry";

const labels: Record<BrowserShortcut, string> = {
  focusAddress: "Focus browser address",
  newTab: "New browser tab",
  closeTab: "Close browser tab",
  reload: "Reload browser page",
  find: "Find in browser page",
  history: "Open browser history",
  back: "Browser back",
  forward: "Browser forward",
  zoomIn: "Browser zoom in",
  zoomOut: "Browser zoom out",
  zoomReset: "Reset browser zoom",
  stop: "Stop loading browser page",
};

// Agent Settings lists the shortcuts the Agent window answers; Editor
// shortcuts stay in Editor Settings.
const scopes: readonly KeymapScope[] = ["Global", "Agent", "Agent chat"];

const agentShortcuts = new Set(
  SETTINGS_KEYMAP_BINDINGS.filter(
    (binding) => !binding.reservedFor && scopes.includes(binding.scope),
  ).map((binding) => binding.id),
);

export default function AgentKeyboardSettings() {
  return (
    <>
      <SettingsKeyboardPage scopes={scopes} visible={agentShortcuts} />
      <section
        className="settings-surface__group"
        aria-labelledby="browser-shortcuts-heading"
      >
        <h3 id="browser-shortcuts-heading">Browser</h3>
        <p>
          Only while focus is inside the browser pane. Esc stops a loading page.
          On a native page Esc leaves annotation mode first, even while idle;
          press Esc again to stop a page that is still loading. These browser
          chords use Control, not Command, on every platform; zoom in also
          accepts the numeric keypad Add key.
        </p>
        <table className="settings-surface__keymap">
          <tbody>
            {BROWSER_SHORTCUT_BINDINGS.map((binding) => (
              <tr key={binding.chord}>
                <th scope="row">
                  {labels[binding.action]}
                  {binding.chord === "F5" ? " (function key)" : ""}
                </th>
                <td>
                  <kbd>{binding.chord}</kbd>
                </td>
              </tr>
            ))}
            <tr>
              <th scope="row">Native page zoom in</th>
              <td>
                <kbd>Ctrl+=</kbd>
                <small>Also Ctrl+Add</small>
              </td>
            </tr>
            <tr>
              <th scope="row">Native page zoom out</th>
              <td>
                <kbd>Ctrl+Subtract</kbd>
              </td>
            </tr>
            <tr>
              <th scope="row">Leave browser annotation</th>
              <td>
                <kbd>Esc</kbd>
                <small>Native page, before Stop loading</small>
              </td>
            </tr>
          </tbody>
        </table>
      </section>
    </>
  );
}
