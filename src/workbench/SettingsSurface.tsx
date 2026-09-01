import { useMemo, useState } from "react";

import { useEditorAppearanceStore } from "../app/editorAppearanceStore";
import {
  EDITOR_FONT_SIZES,
  EDITOR_TAB_SIZES,
} from "../features/files/editorAppearance";

const SETTING_SEARCH_TERMS = [
  "editor",
  "appearance",
  "font",
  "tab",
  "word wrap",
];

export function SettingsSurface() {
  const [query, setQuery] = useState("");
  const fontSize = useEditorAppearanceStore((state) => state.fontSize);
  const tabSize = useEditorAppearanceStore((state) => state.tabSize);
  const wordWrap = useEditorAppearanceStore((state) => state.wordWrap);
  const setAppearance = useEditorAppearanceStore(
    (state) => state.setAppearance,
  );
  const visible = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return (
      !normalized ||
      SETTING_SEARCH_TERMS.some((term) => term.includes(normalized))
    );
  }, [query]);

  return (
    <section className="settings-surface" aria-label="Settings">
      <header>
        <h1>Settings</h1>
        <label>
          Search settings
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
          <h2 id="editor-appearance-heading">Editor Appearance</h2>
          <label>
            Font size
            <select
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
          </label>
          <label>
            Tab size
            <select
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
          </label>
          <label className="settings-surface__toggle">
            <input
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
            Word wrap
          </label>
        </section>
      ) : (
        <p role="status">No matching settings.</p>
      )}
    </section>
  );
}
