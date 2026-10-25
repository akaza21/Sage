import { type ReactNode } from "react";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import {
  EDITOR_SETTING_GROUPS,
  EDITOR_SETTINGS,
  type BooleanSettingSpec,
  type ChoiceSettingSpec,
  type EditorSettingKey,
  type EditorSettings,
  type IntegerSettingSpec,
} from "../../domain/editorSettingsCatalogue";
import { usesMetaModifier } from "../../ui/shortcutLabel";

/** `editor.fontSize` becomes `editor-font-size`. */
function controlId(key: EditorSettingKey) {
  return `editor-${key
    .slice("editor.".length)
    .replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`;
}

function withUnit(value: number, unit: string) {
  return unit === "px" ? `${value}px` : `${value} ${unit}`;
}

function SettingGroup({
  id,
  title,
  children,
}: {
  id: string;
  title: string;
  children: ReactNode;
}) {
  return (
    <section
      className="settings-surface__group"
      role="group"
      aria-labelledby={id}
    >
      <h3 id={id}>{title}</h3>
      {children}
    </section>
  );
}

function SettingCopy({
  id,
  label,
  description,
  defaultLabel,
}: {
  id: string;
  label: string;
  description: string;
  defaultLabel: string;
}) {
  return (
    <div className="settings-surface__copy">
      <label htmlFor={id}>{label}</label>
      <p id={`${id}-description`}>{description}</p>
      <small id={`${id}-default`}>Default: {defaultLabel}</small>
    </div>
  );
}

function ToggleSetting({
  spec,
  checked,
  disabled,
  onChange,
}: {
  spec: BooleanSettingSpec;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = controlId(spec.key as EditorSettingKey);
  return (
    <div className="settings-surface__row">
      <SettingCopy
        id={id}
        label={spec.label}
        description={spec.description}
        defaultLabel={spec.default ? "On" : "Off"}
      />
      <div className="settings-surface__toggle">
        <span>{checked ? "On" : "Off"}</span>
        <input
          id={id}
          type="checkbox"
          checked={checked}
          disabled={disabled}
          aria-describedby={`${id}-description ${id}-default`}
          onChange={(event) => onChange(event.target.checked)}
        />
      </div>
    </div>
  );
}

function SelectSetting({
  spec,
  value,
  disabled,
  onChange,
}: {
  spec: IntegerSettingSpec | ChoiceSettingSpec;
  value: number | string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const id = controlId(spec.key as EditorSettingKey);
  const label = (choice: number | string) =>
    spec.kind === "choice"
      ? spec.choiceLabels[choice][usesMetaModifier() ? 1 : 0]
      : withUnit(Number(choice), spec.unit);
  return (
    <div className="settings-surface__row">
      <SettingCopy
        id={id}
        label={spec.label}
        description={spec.description}
        defaultLabel={label(spec.default)}
      />
      <select
        className="select"
        id={id}
        value={value}
        disabled={disabled}
        aria-describedby={`${id}-description ${id}-default`}
        onChange={(event) => onChange(event.target.value)}
      >
        {spec.choices.map((choice: number | string) => (
          <option key={choice} value={choice}>
            {label(choice)}
          </option>
        ))}
      </select>
    </div>
  );
}

/**
 * The Editor settings page, rendered from the catalogue: each group with a
 * visible entry, in catalogue order. A new setting needs a registry entry and,
 * only for a new kind of value, a new control here.
 */
export function SettingsEditorPage({
  visible,
  controlsDisabled,
}: {
  visible: Set<string>;
  controlsDisabled: boolean;
}) {
  const settings = useEditorSettingsStore((state) => state.settings);
  const setSettings = useEditorSettingsStore((state) => state.setSettings);
  const reset = useEditorSettingsStore((state) => state.reset);
  const atDefaults = EDITOR_SETTINGS.every(
    (spec) => settings[spec.key] === spec.default,
  );
  return (
    <section
      className="settings-surface__section"
      aria-labelledby="editor-appearance-heading"
    >
      <header className="settings-surface__section-header">
        <h2 id="editor-appearance-heading">Editor Appearance</h2>
        <p>These choices apply to every file you open.</p>
      </header>
      {EDITOR_SETTING_GROUPS.map((group) => {
        const specs = EDITOR_SETTINGS.filter(
          (spec) => spec.group === group && visible.has(spec.key),
        );
        if (specs.length === 0) return null;
        return (
          <SettingGroup
            key={group}
            id={`editor-${group.toLowerCase()}-heading`}
            title={group}
          >
            {specs.map((spec) =>
              spec.kind === "boolean" ? (
                <ToggleSetting
                  key={spec.key}
                  spec={spec}
                  checked={settings[spec.key]}
                  disabled={controlsDisabled}
                  onChange={(checked) => setSettings({ [spec.key]: checked })}
                />
              ) : (
                <SelectSetting
                  key={spec.key}
                  spec={spec}
                  value={settings[spec.key]}
                  disabled={controlsDisabled}
                  // The store admits the value, as it does every value.
                  onChange={(value) =>
                    setSettings({
                      [spec.key]:
                        spec.kind === "choice" ? value : Number(value),
                    } as Partial<EditorSettings>)
                  }
                />
              ),
            )}
          </SettingGroup>
        );
      })}
      {visible.has("reset") ? (
        <footer className="settings-surface__footer">
          <button
            className="button button--compact"
            type="button"
            disabled={controlsDisabled || atDefaults}
            onClick={reset}
          >
            Reset Editor Appearance
          </button>
        </footer>
      ) : null}
    </section>
  );
}
