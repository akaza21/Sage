import { useMemo, useState } from "react";

import { useEditorAppearanceStore } from "../app/editorAppearanceStore";
import {
  DEFAULT_EDITOR_APPEARANCE,
  EDITOR_FONT_SIZES,
  EDITOR_TAB_SIZES,
} from "../features/files/editorAppearance";

const SETTING_SEARCH_TERMS = [
  "editor",
  "appearance",
  "font",
  "tab",
  "word wrap",
  "default",
  "reset",
];

export function SettingsSurface() {
  const [query, setQuery] = useState("");
  const fontSize = useEditorAppearanceStore((state) => state.fontSize);
  const tabSize = useEditorAppearanceStore((state) => state.tabSize);
  const wordWrap = useEditorAppearanceStore((state) => state.wordWrap);
  const setAppearance = useEditorAppearanceStore(
    (state) => state.setAppearance,
  );
  const reset = useEditorAppearanceStore((state) => state.reset);
  const atDefaults =
    fontSize === DEFAULT_EDITOR_APPEARANCE.fontSize &&
    tabSize === DEFAULT_EDITOR_APPEARANCE.tabSize &&
    wordWrap === DEFAULT_EDITOR_APPEARANCE.wordWrap;
  const visible = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return (
      !normalized ||
      SETTING_SEARCH_TERMS.some((term) => term.includes(normalized))
    );
  }, [query]);

  return (
    <section className="settings-surface" aria-label="Settings">
      <header className="settings-surface__header">
        <div className="settings-surface__title">
          <h1>Settings</h1>
          <p>Adjust how files are displayed in the Editor.</p>
        </div>
        <label className="settings-surface__search">
          <span>Search settings</span>
          <input
            autoFocus
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search editor settings"
          />
        </label>
      </header>
      {visible ? (
        <section
          className="settings-surface__section"
          aria-labelledby="editor-appearance-heading"
        >
          <header className="settings-surface__section-header">
            <h2 id="editor-appearance-heading">Editor Appearance</h2>
            <p>These choices apply to every file you open.</p>
          </header>
          <div className="settings-surface__row">
            <div className="settings-surface__copy">
              <label htmlFor="editor-font-size">Font size</label>
              <p>Controls the size of text in the code editor.</p>
              <small>Default: {DEFAULT_EDITOR_APPEARANCE.fontSize}px</small>
            </div>
            <select
              id="editor-font-size"
              value={fontSize}
              onChange={(event) =>
                setAppearance({
                  fontSize: Number(event.target.value) as typeof fontSize,
                  tabSize,
                  wordWrap,
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
          <div className="settings-surface__row">
            <div className="settings-surface__copy">
              <label htmlFor="editor-tab-size">Tab size</label>
              <p>Sets the number of spaces used for indentation.</p>
              <small>Default: {DEFAULT_EDITOR_APPEARANCE.tabSize} spaces</small>
            </div>
            <select
              id="editor-tab-size"
              value={tabSize}
              onChange={(event) =>
                setAppearance({
                  fontSize,
                  tabSize: Number(event.target.value) as typeof tabSize,
                  wordWrap,
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
          <div className="settings-surface__row">
            <div className="settings-surface__copy">
              <span>Word wrap</span>
              <p>Wraps long lines so they remain inside the Editor window.</p>
              <small>
                Default: {DEFAULT_EDITOR_APPEARANCE.wordWrap ? "On" : "Off"}
              </small>
            </div>
            <label className="settings-surface__toggle">
              <span>{wordWrap ? "On" : "Off"}</span>
              <input
                aria-label="Word wrap"
                type="checkbox"
                checked={wordWrap}
                onChange={(event) =>
                  setAppearance({
                    fontSize,
                    tabSize,
                    wordWrap: event.target.checked,
                  })
                }
              />
            </label>
          </div>
          <footer className="settings-surface__footer">
            <button type="button" disabled={atDefaults} onClick={reset}>
              Reset Editor Appearance
            </button>
          </footer>
        </section>
      ) : (
        <p role="status">No matching settings.</p>
      )}
    </section>
  );
}
