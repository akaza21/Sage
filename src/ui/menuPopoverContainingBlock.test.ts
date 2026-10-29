import { afterEach, expect, it } from "vitest";

import { inFixedContainingBlock } from "./menuPopoverPosition";

afterEach(() => {
  document.body.replaceChildren();
});

function mount(style: Partial<CSSStyleDeclaration>) {
  const dialog = document.createElement("div");
  Object.assign(dialog.style, style);
  dialog.getBoundingClientRect = () =>
    ({ left: 25, top: 61, right: 625, bottom: 461 }) as DOMRect;
  const popover = document.createElement("div");
  dialog.append(popover);
  document.body.append(dialog);
  return popover;
}

it("shifts a viewport position into a transformed dialog's box", () => {
  const popover = mount({ transform: "translateY(2px)" });
  expect(
    inFixedContainingBlock(
      { left: 56, top: 275, visibility: "visible" },
      popover,
    ),
  ).toEqual({ left: 31, top: 214, visibility: "visible" });
});

it("leaves a position alone when no ancestor contains fixed elements", () => {
  const popover = mount({});
  const position = { left: 56, top: 275 };
  expect(inFixedContainingBlock(position, popover)).toBe(position);
});
