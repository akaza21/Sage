import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { SearchablePicker } from "../../../src/ui/SearchablePicker";
import { Folder } from "../../../src/ui/leanIcons";

// A normal block host also exercises callers whose layout has spare space.
// The shared trigger must not stretch its label or distribute that space.
export function mountPickerTriggerGeometryFixture(
  names: readonly string[],
  disabled: boolean,
) {
  const host = document.createElement("section");
  host.setAttribute("aria-label", "Shared picker geometry");
  document.getElementById("root")!.prepend(host);
  const root = createRoot(host);
  flushSync(() =>
    root.render(
      <>
        {names.map((name) => (
          <SearchablePicker
            key={name}
            ariaLabel={name}
            candidates={[{ id: "selected", label: name }]}
            triggerIcon={<Folder size={16} aria-hidden="true" />}
            selectedId="selected"
            disabled={disabled}
            onSelect={() => {}}
          />
        ))}
      </>,
    ),
  );
}
