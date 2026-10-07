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
  return (
    <nav className="settings-surface__nav" aria-label="Settings pages">
      <div className="settings-surface__nav-list" role="tablist">
        {SETTINGS_PAGES.map((page) => (
          <button
            className="settings-surface__nav-item"
            key={page.id}
            type="button"
            role="tab"
            aria-selected={page.id === activePageId}
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
