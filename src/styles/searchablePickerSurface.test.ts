import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

it("shrinks the nested picker list into the bounded panel without shrinking its controls", async () => {
  const style = document.createElement("style");
  style.textContent =
    (await readFile("src/styles/shared-controls.css", "utf8")) +
    (await readFile("src/styles/searchable-picker.css", "utf8")) +
    (await readFile(
      "src/styles/searchable-picker-bounded-popover.css",
      "utf8",
    ));
  const panel = document.createElement("div");
  panel.className =
    "menu-popover menu-popover--bounded searchable-picker__popover";
  panel.innerHTML =
    '<input class="searchable-picker__input"><p class="searchable-picker__current">Current choice</p><div role="listbox"></div><p class="searchable-picker__footer">Session scope</p>';
  document.head.append(style);
  document.body.append(panel);
  try {
    expect(getComputedStyle(panel).display).toBe("flex");
    expect(getComputedStyle(panel).flexDirection).toBe("column");
    const list = panel.querySelector('[role="listbox"]')!;
    expect(getComputedStyle(list).flexShrink).toBe("1");
    expect(getComputedStyle(list).minHeight).toBe("28px");
    expect(getComputedStyle(list).overflowY).toBe("auto");
    for (const control of panel.querySelectorAll("input, p"))
      expect(getComputedStyle(control).flexShrink).toBe("0");
  } finally {
    panel.remove();
    style.remove();
  }
});
