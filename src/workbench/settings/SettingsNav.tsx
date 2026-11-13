import { useRef } from "react";

import { handleTabListKeyDown } from "../../ui/tabListKeys";
import {
  SETTINGS_PAGES,
  type SettingsPageId,
} from "../../domain/settingsNavigation";

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
        // The Browser page opens elsewhere, so arrowing past it must not
        // select it: arrows move focus and Enter or Space selects.
        onKeyDown={(event) =>
          handleTabListKeyDown(event, listRef.current, "horizontal", "manual")
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
