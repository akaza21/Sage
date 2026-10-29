/**
 * The browser menu's settings items. Browser settings opens Settings on its
 * Browser page, as Codex does; Site settings keeps this tab's saved site
 * permissions in reach, since those are per tab and not on that page.
 */
export function BrowserSettingsMenuItems({
  run,
  onSettings,
}: {
  run: (action: () => void) => void;
  onSettings: (target: "browser" | "site") => void;
}) {
  return (
    <>
      <button
        role="menuitem"
        type="button"
        onClick={() => run(() => onSettings("browser"))}
      >
        Browser settings
      </button>
      <button
        role="menuitem"
        type="button"
        onClick={() => run(() => onSettings("site"))}
      >
        Site settings
      </button>
    </>
  );
}
