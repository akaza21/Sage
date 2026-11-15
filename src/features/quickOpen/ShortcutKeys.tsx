import { shortcutKeys } from "./quickOpenDomain";

/** The outer <kbd> is the whole combination, so it reads as one "Ctrl+Shift+P". */
export function ShortcutKeys({ label }: { label: string }) {
  const keys = shortcutKeys(label);
  return (
    <kbd className="quick-open__shortcut" aria-label={label}>
      {keys.map((key, index) => (
        <span key={index} className="quick-open__shortcut-part">
          {index > 0 ? (
            <span className="quick-open__shortcut-plus" aria-hidden="true">
              +
            </span>
          ) : null}
          <kbd className="keycap">{key}</kbd>
        </span>
      ))}
    </kbd>
  );
}
