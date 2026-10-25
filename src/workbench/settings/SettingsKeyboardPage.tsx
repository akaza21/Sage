import {
  KEYMAP_BINDINGS,
  formatKeymapChords,
  type KeymapScope,
} from "../../domain/keymapRegistry";
import { usesMetaModifier } from "../../ui/shortcutLabel";

const SCOPES: readonly KeymapScope[] = [
  "Global",
  "Agent",
  "Agent chat",
  "Editor",
];

const SCOPE_COPY: Record<KeymapScope, string> = {
  Global: "Available anywhere in Sage.",
  Agent: "Available while the Agents view is open.",
  "Agent chat": "Available in the focused chat pane, including its composer.",
  Editor: "Available while a file is open in the Editor.",
};

export function SettingsKeyboardPage({ visible }: { visible: Set<string> }) {
  const apple = usesMetaModifier();
  return (
    <section
      className="settings-surface__section"
      aria-labelledby="keyboard-shortcuts-heading"
    >
      <header className="settings-surface__section-header">
        <h2 id="keyboard-shortcuts-heading">Keyboard Shortcuts</h2>
        <p>These shortcuts are read-only for now.</p>
      </header>
      {SCOPES.map((scope) => {
        const bindings = KEYMAP_BINDINGS.filter(
          (binding) => binding.scope === scope && visible.has(binding.id),
        );
        if (bindings.length === 0) return null;
        const headingId = `keyboard-${scope.toLowerCase().replaceAll(" ", "-")}-heading`;
        return (
          <section
            className="settings-surface__group"
            role="group"
            aria-labelledby={headingId}
            key={scope}
          >
            <h3 id={headingId}>{scope}</h3>
            <p className="settings-surface__group-note">{SCOPE_COPY[scope]}</p>
            <table className="settings-surface__keymap">
              <tbody>
                {bindings.map((binding) => (
                  <tr key={binding.id}>
                    <th scope="row">
                      <span>{binding.label}</span>
                      <small>{binding.description}</small>
                    </th>
                    <td>
                      {formatKeymapChords(binding, apple).map((chord) => (
                        <kbd key={chord}>{chord}</kbd>
                      ))}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        );
      })}
    </section>
  );
}
