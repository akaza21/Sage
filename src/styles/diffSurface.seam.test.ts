import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Diff surface stylesheet seam", () => {
  it("fits the pane, keeps line numbers in view and keeps hunk headers neutral", async () => {
    const css = await readFile(resolve("src/styles/diff-surface.css"), "utf8");
    const rule = (selector: string) =>
      css.match(
        new RegExp(`^${selector.replace(/[.>]/g, "\\$&")} \\{([^}]*)\\}`, "m"),
      )?.[1] ?? "";
    // A fixed minimum width made every narrow pane scroll sideways.
    expect(css).not.toMatch(/min-width:\s*720px/);
    expect(rule(".diff-editor")).toMatch(
      /grid-template-columns: minmax\(max-content, 1fr\)/,
    );
    expect(rule(".diff-line > span")).toMatch(/position: sticky/);
    expect(rule(".diff-line__comment-gutter")).toMatch(/position: sticky/);
    // The @@ header is context, not an added line.
    expect(rule(".diff-hunk__header")).not.toMatch(/--color-diff-add/);
    expect(rule(".diff-hunk__header")).toMatch(/contain: inline-size/);
  });
});
