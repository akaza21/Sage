import { useRef } from "react";

import { handleTabListKeyDown } from "../../ui/tabListKeys";
import {
  SETTINGS_PAGES,
  type SettingsPageId,
} from "../../domain/settingsNavigation";

/** The Editor settings page list, a column beside the rows like VS Code's. */
export function SettingsNav({
  activePageId,
  matchingPageIds,
  onSelect,
}: {
  activePageId: SettingsPageId;
  matchingPageIds: Set<SettingsPageId>;
  onSelect: (pageId: SettingsPageId) => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  return (
    <nav className="settings-surface__nav" aria-label="Settings pages">
      <div
        className="settings-surface__nav-list"
        ref={listRef}
        role="tablist"
        aria-orientation="vertical"
        onKeyDown={(event) =>
          handleTabListKeyDown(event, listRef.current, "vertical", "automatic")
        }
      >
        {SETTINGS_PAGES.map((page) => (
          <button
            className="settings-surface__nav-item"
            key={page.id}
            type="button"
            role="tab"
            aria-selected={page.id === activePageId}
            tabIndex={page.id === activePageId ? 0 : -1}
            data-matches={matchingPageIds.has(page.id)}
            onClick={() => onSelect(page.id)}
          >
            {page.label}
          </button>
        ))}
      </div>
    </nav>
  );
}
