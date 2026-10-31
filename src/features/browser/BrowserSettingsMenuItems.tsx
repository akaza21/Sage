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
