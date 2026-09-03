import { useMemo, useState, type ReactNode } from "react";

import { useEditorAppearanceStore } from "../app/editorAppearanceStore";
import {
  DEFAULT_EDITOR_APPEARANCE,
  EDITOR_FONT_SIZES,
  EDITOR_TAB_SIZES,
  type EditorAppearance,
} from "../features/files/editorAppearance";

type SettingKey =
  "fontSize" | "tabSize" | "wordWrap" | "lineNumbers" | "showWhitespace";

const SETTING_COPY: Record<
  SettingKey,
  { label: string; description: string; group: string }
> = {
  fontSize: {
    label: "Font size",
    description: "Controls the size of text in the code editor.",
    group: "Editor Display",
  },
  tabSize: {
    label: "Tab size",
    description: "Sets the number of spaces used for indentation.",
    group: "Indentation",
  },
  wordWrap: {
    label: "Word wrap",
    description: "Wraps long lines so they remain inside the Editor window.",
    group: "Editor Display",
  },
  lineNumbers: {
    label: "Line numbers",
    description: "Shows a numbered gutter beside each line.",
    group: "Editor Display",
  },
  showWhitespace: {
    label: "Show whitespace",
    description: "Reveals spaces and tabs when indentation is unclear.",
    group: "Editor Display",
  },
};

function matches(query: string, key: SettingKey) {
  const copy = SETTING_COPY[key];
  return (
    !query ||
    `${copy.label} ${copy.description} ${copy.group}`
      .toLowerCase()
      .includes(query)
  );
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
  label,
  description,
  checked,
  defaultChecked,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  defaultChecked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="settings-surface__row">
      <div className="settings-surface__copy">
        <label htmlFor={id}>{label}</label>
        <p id={`${id}-description`}>{description}</p>
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

export function SettingsSurface() {
  const [query, setQuery] = useState("");
  const fontSize = useEditorAppearanceStore((state) => state.fontSize);
  const tabSize = useEditorAppearanceStore((state) => state.tabSize);
  const wordWrap = useEditorAppearanceStore((state) => state.wordWrap);
  const lineNumbers = useEditorAppearanceStore((state) => state.lineNumbers);
  const showWhitespace = useEditorAppearanceStore(
    (state) => state.showWhitespace,
  );
  const persistenceMessage = useEditorAppearanceStore(
    (state) => state.persistenceMessage,
  );
  const persistenceClosing = useEditorAppearanceStore(
    (state) => state.persistenceClosing,
  );
  const persistenceReady = useEditorAppearanceStore(
    (state) => state.persistenceReady,
  );
  const persistencePhase = useEditorAppearanceStore(
    (state) => state.persistencePhase,
  );
  const setAppearance = useEditorAppearanceStore(
    (state) => state.setAppearance,
  );
  const retryPersistence = useEditorAppearanceStore(
    (state) => state.retryPersistence,
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
  const visibility = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return {
      fontSize: matches(normalized, "fontSize"),
      tabSize: matches(normalized, "tabSize"),
      wordWrap: matches(normalized, "wordWrap"),
      lineNumbers: matches(normalized, "lineNumbers"),
      showWhitespace: matches(normalized, "showWhitespace"),
      reset:
        !normalized || "reset editor appearance defaults".includes(normalized),
    };
  }, [query]);
  const showDisplay =
    visibility.fontSize ||
    visibility.wordWrap ||
    visibility.lineNumbers ||
    visibility.showWhitespace;
  const hasResults = showDisplay || visibility.tabSize || visibility.reset;
  const update = (change: Partial<EditorAppearance>) =>
    setAppearance({ ...appearance, ...change });
  const controlsDisabled = !persistenceReady || persistenceClosing;

  return (
    <section
      className="settings-surface"
      aria-label="Settings"
      aria-busy={persistencePhase === "loading"}
    >
      <header className="settings-surface__header">
        <div className="settings-surface__title">
          <h1>Settings</h1>
          <p>Adjust how files are displayed in the Editor.</p>
        </div>
        <label className="settings-surface__search">
          <span>Search settings</span>
          <input
            autoFocus
            className="input"
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search editor settings"
          />
        </label>
      </header>
      {persistencePhase === "loading" ? (
        <p className="settings-surface__loading" role="status">
          Loading editor settings…
        </p>
      ) : null}
      {persistenceMessage ? (
        <div className="settings-surface__error" role="alert">
          <span>{persistenceMessage}</span>
          <button
            className="button button--compact"
            type="button"
            disabled={persistenceClosing}
            onClick={retryPersistence}
          >
            Retry
          </button>
        </div>
      ) : null}
      {hasResults ? (
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
              {visibility.fontSize ? (
                <div className="settings-surface__row">
                  <div className="settings-surface__copy">
                    <label htmlFor="editor-font-size">
                      {SETTING_COPY.fontSize.label}
                    </label>
                    <p id="editor-font-size-description">
                      {SETTING_COPY.fontSize.description}
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
              {visibility.lineNumbers ? (
                <ToggleSetting
                  id="editor-line-numbers"
                  label={SETTING_COPY.lineNumbers.label}
                  description={SETTING_COPY.lineNumbers.description}
                  checked={lineNumbers}
                  defaultChecked={DEFAULT_EDITOR_APPEARANCE.lineNumbers}
                  disabled={controlsDisabled}
                  onChange={(checked) => update({ lineNumbers: checked })}
                />
              ) : null}
              {visibility.wordWrap ? (
                <ToggleSetting
                  id="editor-word-wrap"
                  label={SETTING_COPY.wordWrap.label}
                  description={SETTING_COPY.wordWrap.description}
                  checked={wordWrap}
                  defaultChecked={DEFAULT_EDITOR_APPEARANCE.wordWrap}
                  disabled={controlsDisabled}
                  onChange={(checked) => update({ wordWrap: checked })}
                />
              ) : null}
              {visibility.showWhitespace ? (
                <ToggleSetting
                  id="editor-show-whitespace"
                  label={SETTING_COPY.showWhitespace.label}
                  description={SETTING_COPY.showWhitespace.description}
                  checked={showWhitespace}
                  defaultChecked={DEFAULT_EDITOR_APPEARANCE.showWhitespace}
                  disabled={controlsDisabled}
                  onChange={(checked) => update({ showWhitespace: checked })}
                />
              ) : null}
            </SettingGroup>
          ) : null}
          {visibility.tabSize ? (
            <SettingGroup id="editor-indentation-heading" title="Indentation">
              <div className="settings-surface__row">
                <div className="settings-surface__copy">
                  <label htmlFor="editor-tab-size">
                    {SETTING_COPY.tabSize.label}
                  </label>
                  <p id="editor-tab-size-description">
                    {SETTING_COPY.tabSize.description}
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
          {visibility.reset ? (
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
      ) : (
        <p className="settings-surface__empty" role="status">
          No matching settings.
        </p>
      )}
    </section>
  );
}
