import { afterEach, expect, it, vi } from "vitest";

import {
  inFixedContainingBlock,
  menuPopoverPosition,
} from "./menuPopoverPosition";

afterEach(() => {
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

it("keeps a composer popover attached to its trigger when a changed-files summary is present", () => {
  document.body.innerHTML =
    '<section class="chat-surface"><div class="chat-dock"><div class="chat-edit-summary-zone">2 files changed</div><form class="composer"><button>Effort</button></form></div></section><div class="composer__effort-menu"></div>';
  const trigger = document.querySelector("button")!;
  const menu = document.querySelector<HTMLElement>(".composer__effort-menu")!;
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this.classList.contains("chat-dock"))
        return new DOMRect(0, 615, 600, 145);
      if (this.classList.contains("chat-edit-summary-zone"))
        return new DOMRect(0, 615, 600, 30);
      if (this === menu) return new DOMRect(0, 0, 232, 100);
      return new DOMRect(400, 720, 24, 28);
    },
  );
  const result = menuPopoverPosition(trigger, menu, trigger, 4, false);
  expect(result.top).toBe(616);
  expect(Number(result.top) + 100 + 4).toBe(720);
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
