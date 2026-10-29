import { lazy, Suspense, useEffect, useMemo, useState } from "react";

import { useEditorSettingsStore } from "../app/editorSettingsStore";
import { useSettingsPageRequest } from "../app/settingsPageRequest";
import {
  SETTINGS_PAGES,
  matchingSettingsEntryIds,
  pagesWithMatches,
  type SettingsPageId,
} from "../domain/settingsNavigation";
import { SettingsEditorPage } from "./settings/SettingsEditorPage";
import { SettingsKeyboardPage } from "./settings/SettingsKeyboardPage";
import { SettingsNav } from "./settings/SettingsNav";

const SettingsBrowserPage = lazy(() =>
  import("./settings/SettingsBrowserPage").then(({ SettingsBrowserPage }) => ({
    default: SettingsBrowserPage,
  })),
);

export function SettingsSurface() {
  const [query, setQuery] = useState("");
  const [activePageId, setActivePageId] = useState<SettingsPageId>(
    () => useSettingsPageRequest.getState().page ?? SETTINGS_PAGES[0].id,
  );
  // A caller such as the browser's menu asks for a page: the first render
  // opens on it, and while Settings is open a new request switches to it with
  // the search cleared. Either way the request is then forgotten.
  useEffect(() => {
    useSettingsPageRequest.setState({ page: null });
    return useSettingsPageRequest.subscribe(({ page }) => {
      if (!page) return;
      setActivePageId(page);
      setQuery("");
      useSettingsPageRequest.setState({ page: null });
    });
  }, []);
  const persistenceMessage = useEditorSettingsStore(
    (state) => state.persistenceMessage,
  );
  const persistenceClosing = useEditorSettingsStore(
    (state) => state.persistenceClosing,
  );
  const persistenceReady = useEditorSettingsStore(
    (state) => state.persistenceReady,
  );
  const persistencePhase = useEditorSettingsStore(
    (state) => state.persistencePhase,
  );
  const retryPersistence = useEditorSettingsStore(
    (state) => state.retryPersistence,
  );
  const visible = useMemo(
    () => matchingSettingsEntryIds(query, activePageId),
    [query, activePageId],
  );
  const matchingPageIds = useMemo(() => pagesWithMatches(query), [query]);
  const activePage = SETTINGS_PAGES.find((page) => page.id === activePageId);
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
          <p>{activePage?.description}</p>
        </div>
        <label className="settings-surface__search">
          <span>Search settings</span>
          <input
            autoFocus
            className="input"
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search settings"
          />
        </label>
      </header>
      <SettingsNav
        activePageId={activePageId}
        matchingPageIds={matchingPageIds}
        onSelect={setActivePageId}
      />
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
      {visible.size === 0 ? (
        <p className="settings-surface__empty" role="status">
          No matching settings.
        </p>
      ) : activePageId === "keyboard" ? (
        <SettingsKeyboardPage visible={visible} />
      ) : activePageId === "browser" ? (
        <Suspense
          fallback={
            <p className="settings-surface__loading" role="status">
              Loading browser settings…
            </p>
          }
        >
          <SettingsBrowserPage visible={visible} />
        </Suspense>
      ) : (
        <SettingsEditorPage
          visible={visible}
          controlsDisabled={controlsDisabled}
        />
      )}
    </section>
  );
}
