import { cleanup, render, screen } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, expect, it, vi } from "vitest";

import { MenuPopover } from "./MenuPopover";
import type * as PositionModule from "./menuPopoverPosition";

// Two placements that each make the other the better fit: every measurement
// returns the opposite top, as a flip that changes the list's room can.
let calls = 0;
vi.mock("./menuPopoverPosition", async (importOriginal) => {
  const actual = await importOriginal<typeof PositionModule>();
  return {
    ...actual,
    menuPopoverPosition: () => {
      calls += 1;
      return { left: 8, top: calls % 2 ? 40 : 200, visibility: "visible" };
    },
  };
});

afterEach(() => {
  cleanup();
  calls = 0;
});

function Harness() {
  const triggerRef = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button ref={triggerRef} type="button">
        Open
      </button>
      <MenuPopover
        id="oscillating-menu"
        ariaLabel="Oscillating menu"
        open
        triggerRef={triggerRef}
        onDismiss={() => undefined}
      >
        <button type="button" role="menuitem">
          Item
        </button>
      </MenuPopover>
    </>
  );
}

it("bounds re-measures when two placements keep flipping", () => {
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  render(<Harness />);
  expect(screen.getByRole("menu", { name: "Oscillating menu" })).toBeVisible();
  expect(calls).toBeLessThan(20);
  expect(
    errors.mock.calls.some((call) =>
      String(call[0]).includes("Maximum update depth"),
    ),
  ).toBe(false);
});
