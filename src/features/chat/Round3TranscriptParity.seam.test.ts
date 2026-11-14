import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

const read = (path: string) => readFileSync(path, "utf8");
const polish = () => read("src/features/chat/chat-transcript-polish.css");

it("leaves the fork option about 33px below the title (R3-12a)", () => {
  expect(read("src/features/chat/fork-chat.css")).toMatch(
    /\.fork-chat-dialog h2\s*\{[^}]*margin:\s*0 var\(--space-1\) calc\(var\(--space-4\) \+ var\(--space-1\)\);/s,
  );
});

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

it("keeps Don't ask again at 13px (R3-12d)", () => {
  expect(read("src/features/chat/transcript-interactions.css")).toMatch(
    /\.close-side-chat-dialog\s*\{[^}]*font-size:\s*13px;/s,
  );
});

it("keeps annotation hover bounds on the chip after sharing compact controls", () => {
  expect(read("src/features/chat/transcript-interactions.css")).toMatch(
    /\.annotation-chip\s*\{[^}]*display:\s*inline-flex;[^}]*width:\s*fit-content;/s,
  );
});

it("draws the initial approval ring without requiring focus-visible (R3-09)", () => {
  expect(read("src/features/chat/acp/permission-prompt.css")).toMatch(
    /button\[data-initial-focus\][^{]*\{[^}]*outline:\s*var\(--focus-ring-width\) solid var\(--color-accent\);[^}]*box-shadow:\s*0 0 0 1px var\(--color-accent\);/s,
  );
});

it("shares the raised surface between fork and selection popovers (R3-07)", () => {
  expect(read("src/features/chat/fork-chat.css")).toMatch(
    /\.answer-more-menu\.menu-popover\s*\{[^}]*background:\s*var\(--color-panel-raised\);/s,
  );
  expect(read("src/features/chat/transcript-interactions.css")).toMatch(
    /\.chat-selection-popover,[^{]*\.transcript-selection-status\s*\{[^}]*background:\s*var\(--color-panel-raised\);/s,
  );
});

it("reserves find space below header actions at every pane width (R3-05)", () => {
  const css = read("src/features/chat/chat-find.css");
  expect(css).toMatch(
    /\.chat-surface--agent:has\(> \.chat-find\)\s*\{[^}]*grid-template:\s*"find" auto\s*"notice" auto\s*"transcript" minmax\(0, 1fr\)\s*"composer" fit-content\(60%\)\s*\/ minmax\(0, 1fr\);/s,
  );
  // The 314 px find box never widens the column past a narrow pane.
  expect(css).toMatch(
    /\.chat-surface--agent > \.chat-find\s*\{[^}]*grid-area:\s*find;/s,
  );
  expect(css).not.toMatch(/position:\s*absolute/);
});

it("uses a compact disclosure gap, inherited label color and an 8px chevron gap (R3-04)", () => {
  expect(read("src/features/chat/acp/acp-activity-details.css")).toMatch(
    /\.acp-tool-rollup__items\s*\{[^}]*margin-top:\s*calc\(var\(--space-1\) \/ 2\);/s,
  );
  expect(polish()).toMatch(
    /\.chat-transcript-work__groups \.acp-tool-rollup > summary\s*\{[^}]*gap:\s*var\(--space-2\);/s,
  );
  expect(read("src/features/chat/acp/acp-activity-cards.css")).toMatch(
    /\.acp-activity-card > summary > span\.acp-tool-rollup__summary-copy\s*\{[^}]*color:\s*inherit;/s,
  );
  expect(polish()).toMatch(
    /\.acp-tool-rollup\[open\] > summary\s*\{[^}]*color:\s*var\(--color-text-emphasis\);/s,
  );
});

it.each([false, true])(
  "inherits the open label color across lazy stylesheet order %s (R3-04)",
  (baseLast) => {
    const style = document.createElement("style");
    const base = read("src/features/chat/acp/acp-activity-cards.css");
    style.textContent =
      `:root { --color-text-muted: #949494; }` +
      (baseLast ? polish() + base : base + polish());
    const groups = document.createElement("div");
    groups.className = "chat-transcript-work__groups";
    groups.innerHTML =
      '<details class="acp-activity-card acp-tool-rollup" open><summary style="color: rgb(255, 255, 255)"><span class="acp-tool-rollup__summary-copy">Explored</span></summary></details>';
    document.head.append(style);
    document.body.append(groups);
    try {
      expect(
        getComputedStyle(
          groups.querySelector(".acp-tool-rollup__summary-copy")!,
        ).color,
      ).toBe("rgb(255, 255, 255)");
    } finally {
      style.remove();
      groups.remove();
    }
  },
);

it("owns the earlier answer gap in the turn layout and keeps time interaction-only (R3-03)", () => {
  expect(read("src/features/chat/acp/acp-chat.css")).toMatch(
    /\.acp-turn__items\s*\{[^}]*gap:\s*var\(--transcript-item-gap\);/s,
  );
  expect(polish()).toMatch(
    /\.chat-transcript-actions__time\s*\{[^}]*opacity:\s*0;/s,
  );
  expect(polish()).toMatch(
    /\.chat-transcript-actions:focus-within > \.chat-transcript-actions__time\s*\{\s*opacity:\s*1;/s,
  );
});
