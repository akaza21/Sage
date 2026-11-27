import { readFileSync } from "node:fs";
import { afterEach, expect, it } from "vitest";

const styles: HTMLStyleElement[] = [];
afterEach(() => {
  styles.splice(0).forEach((style) => style.remove());
  document.body.replaceChildren();
});

function loadStyle(path: string) {
  const style = document.createElement("style");
  style.textContent = readFileSync(path, "utf8");
  document.head.append(style);
  styles.push(style);
}

/** The used font size, reading a token through the root like the browser. */
function fontSize(element: Element) {
  const value = getComputedStyle(element).fontSize;
  const token = /^var\((--[\w-]+)\)$/u.exec(value)?.[1];
  return token
    ? getComputedStyle(document.documentElement).getPropertyValue(token).trim()
    : value;
}

it("sets selection popover labels and the close dialog's copy at 13px (B5, B7, R3-12d)", () => {
  loadStyle("src/styles/global.css");
  loadStyle("src/features/chat/transcript-interactions.css");
  document.body.innerHTML = `
    <div class="chat-selection-popover"><button>Quote</button></div>
    <dialog class="close-side-chat-dialog" open>
      <label><input type="checkbox" /> Don't ask again</label>
    </dialog>`;
  const label = document.querySelector(".chat-selection-popover button")!;
  const dialog = document.querySelector(".close-side-chat-dialog")!;
  expect(fontSize(label)).toBe("13px");
  expect(getComputedStyle(label).paddingLeft).toBe("7px");
  expect(fontSize(dialog)).toBe("13px");
});
