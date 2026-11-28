import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

it("lets a dock summary inherit the slot width without a second scrollbar reservation", async () => {
  const style = document.createElement("style");
  style.textContent = await readFile("src/styles/chat-surface.css", "utf8");
  const slot = document.createElement("div");
  slot.className = "chat-edit-summary-zone";
  document.head.append(style);
  document.body.append(slot);
  try {
    const css = getComputedStyle(slot);
    expect(css.getPropertyValue("--edit-summary-width")).toBe("100%");
    expect(css.getPropertyValue("--edit-summary-margin")).toBe("0");
    expect(css.overflowY).toBe("auto");
  } finally {
    slot.remove();
    style.remove();
  }
});
