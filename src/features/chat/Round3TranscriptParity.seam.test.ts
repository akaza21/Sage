import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

const read = (path: string) => readFileSync(path, "utf8");

it("keeps both scrim dialog titles at 18px after the base dialog rule (R3-12a)", () => {
  const style = document.createElement("style");
  style.textContent = read("src/styles/shared-dialogs.css");
  const dialog = document.createElement("dialog");
  dialog.className = "dirty-dialog dialog--scrim";
  dialog.innerHTML = "<h2>Fork chat from here</h2>";
  document.head.append(style);
  document.body.append(dialog);
  try {
    expect(getComputedStyle(dialog.firstElementChild!).fontSize).toBe("18px");
    expect(getComputedStyle(dialog.firstElementChild!).fontWeight).toBe("600");
  } finally {
    style.remove();
    dialog.remove();
  }
});
