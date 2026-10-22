import { useRef, useState } from "react";
import { MenuPopover } from "../../../src/ui/MenuPopover";
import { SearchablePicker } from "../../../src/ui/SearchablePicker";

export function NestedPopoverFixture() {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button ref={trigger} type="button" onClick={() => setOpen(true)}>
        Nested fixture
      </button>
      <MenuPopover
        id="nested-fixture"
        ariaLabel="Nested fixture parent"
        role="dialog"
        open={open}
        triggerRef={trigger}
        onDismiss={() => setOpen(false)}
        className="composer__popover"
      >
        <SearchablePicker
          ariaLabel="Nested fixture picker"
          candidates={[
            { id: "first", label: "First option" },
            { id: "second", label: "Second option" },
          ]}
          selectedId="first"
          onSelect={() => {}}
          searchable={false}
          popoverClassName="composer__popover"
        />
      </MenuPopover>
    </>
  );
}
