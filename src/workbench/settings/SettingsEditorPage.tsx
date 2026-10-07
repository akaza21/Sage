import { type ReactNode } from "react";

import { useEditorAppearanceStore } from "../../app/editorAppearanceStore";
import {
  DEFAULT_EDITOR_APPEARANCE,
  EDITOR_FONT_SIZES,
  EDITOR_TAB_SIZES,
  type EditorAppearance,
} from "../../features/files/editorAppearance";
import { SETTINGS_PAGES } from "../../domain/settingsNavigation";

type SettingKey = keyof EditorAppearance;

const COPY = new Map(
  (SETTINGS_PAGES.find((page) => page.id === "editor")?.entries ?? []).map(
    (entry) => [entry.id, entry],
  ),
);

function copyFor(id: string) {
  const entry = COPY.get(id);
  if (!entry) throw new Error(`unknown editor setting: ${id}`);
  return entry;
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

function ToggleSetting({
  id,
  settingKey,
  checked,
  disabled,
  onChange,
}: {
  id: string;
  settingKey: SettingKey;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  const copy = copyFor(settingKey);
  const defaultChecked = DEFAULT_EDITOR_APPEARANCE[settingKey] === true;
  return (
    <div className="settings-surface__row">
      <div className="settings-surface__copy">
        <label htmlFor={id}>{copy.label}</label>
        <p id={`${id}-description`}>{copy.description}</p>
        <small id={`${id}-default`}>
          Default: {defaultChecked ? "On" : "Off"}
        </small>
      </div>
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

export function SettingsEditorPage({
  visible,
  controlsDisabled,
}: {
  visible: Set<string>;
  controlsDisabled: boolean;
}) {
  const fontSize = useEditorAppearanceStore((state) => state.fontSize);
  const tabSize = useEditorAppearanceStore((state) => state.tabSize);
  const wordWrap = useEditorAppearanceStore((state) => state.wordWrap);
  const lineNumbers = useEditorAppearanceStore((state) => state.lineNumbers);
  const showWhitespace = useEditorAppearanceStore(
    (state) => state.showWhitespace,
  );
  const setAppearance = useEditorAppearanceStore(
    (state) => state.setAppearance,
  );
  const reset = useEditorAppearanceStore((state) => state.reset);
  const appearance: EditorAppearance = {
    fontSize,
    tabSize,
    wordWrap,
    lineNumbers,
    showWhitespace,
  };
  const atDefaults = Object.entries(DEFAULT_EDITOR_APPEARANCE).every(
    ([key, value]) => appearance[key as SettingKey] === value,
  );
  const update = (change: Partial<EditorAppearance>) =>
    setAppearance({ ...appearance, ...change });
  const showDisplay =
    visible.has("fontSize") ||
    visible.has("wordWrap") ||
    visible.has("lineNumbers") ||
    visible.has("showWhitespace");
  return (
    <section
      className="settings-surface__section"
      aria-labelledby="editor-appearance-heading"
    >
      <header className="settings-surface__section-header">
        <h2 id="editor-appearance-heading">Editor Appearance</h2>
        <p>These choices apply to every file you open.</p>
      </header>
      {showDisplay ? (
        <SettingGroup id="editor-display-heading" title="Editor Display">
          {visible.has("fontSize") ? (
            <div className="settings-surface__row">
              <div className="settings-surface__copy">
                <label htmlFor="editor-font-size">
                  {copyFor("fontSize").label}
                </label>
                <p id="editor-font-size-description">
                  {copyFor("fontSize").description}
                </p>
                <small id="editor-font-size-default">
                  Default: {DEFAULT_EDITOR_APPEARANCE.fontSize}px
                </small>
              </div>
              <select
                className="select"
                id="editor-font-size"
                value={fontSize}
                disabled={controlsDisabled}
                aria-describedby="editor-font-size-description editor-font-size-default"
                onChange={(event) =>
                  update({
                    fontSize: Number(event.target.value) as typeof fontSize,
                  })
                }
              >
                {EDITOR_FONT_SIZES.map((size) => (
                  <option key={size} value={size}>
                    {size}px
                  </option>
                ))}
              </select>
            </div>
          ) : null}
          {visible.has("lineNumbers") ? (
            <ToggleSetting
              id="editor-line-numbers"
              settingKey="lineNumbers"
              checked={lineNumbers}
              disabled={controlsDisabled}
              onChange={(checked) => update({ lineNumbers: checked })}
            />
          ) : null}
          {visible.has("wordWrap") ? (
            <ToggleSetting
              id="editor-word-wrap"
              settingKey="wordWrap"
              checked={wordWrap}
              disabled={controlsDisabled}
              onChange={(checked) => update({ wordWrap: checked })}
            />
          ) : null}
          {visible.has("showWhitespace") ? (
            <ToggleSetting
              id="editor-show-whitespace"
              settingKey="showWhitespace"
              checked={showWhitespace}
              disabled={controlsDisabled}
              onChange={(checked) => update({ showWhitespace: checked })}
            />
          ) : null}
        </SettingGroup>
      ) : null}
      {visible.has("tabSize") ? (
        <SettingGroup id="editor-indentation-heading" title="Indentation">
          <div className="settings-surface__row">
            <div className="settings-surface__copy">
              <label htmlFor="editor-tab-size">
                {copyFor("tabSize").label}
              </label>
              <p id="editor-tab-size-description">
                {copyFor("tabSize").description}
              </p>
              <small id="editor-tab-size-default">
                Default: {DEFAULT_EDITOR_APPEARANCE.tabSize} spaces
              </small>
            </div>
            <select
              className="select"
              id="editor-tab-size"
              value={tabSize}
              disabled={controlsDisabled}
              aria-describedby="editor-tab-size-description editor-tab-size-default"
              onChange={(event) =>
                update({
                  tabSize: Number(event.target.value) as typeof tabSize,
                })
              }
            >
              {EDITOR_TAB_SIZES.map((size) => (
                <option key={size} value={size}>
                  {size} spaces
                </option>
              ))}
            </select>
          </div>
        </SettingGroup>
      ) : null}
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
