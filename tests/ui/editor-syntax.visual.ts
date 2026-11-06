import { expect, test, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// What the editor actually paints for representative syntax in Chromium:
// every highlighted token takes its colour from the document syntax tokens
// (--doc-syntax-*), and Markdown keeps CodeMirror's default treatments.

const TYPESCRIPT = [
  "// note",
  "class Box<T> {",
  "  size = 2;",
  "}",
  'const label: string = "text";',
  "const pattern = /ab+c/;",
  "let flag = true;",
  "@decorated",
  "",
].join("\n");

const MARKDOWN =
  "# Title\n\nSome *soft* and **loud** text with [a link](https://example.com).\n";

type SyntaxToken =
  | "meta"
  | "keyword"
  | "atom"
  | "literal"
  | "string"
  | "regexp"
  | "variable-definition"
  | "type"
  | "comment";

interface Painted {
  color: string;
  fontStyle: string;
  fontWeight: string;
  textDecorationLine: string;
}

async function replaceDocument(page: Page, editorName: string, text: string) {
  const content = page.getByRole("textbox", { name: editorName });
  await content.click();
  await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.insertText(text);
}

/** The colour each token resolves to on the editor's own content. */
async function tokenColors(page: Page, tokens: readonly SyntaxToken[]) {
  return page.locator(".file-surface .cm-content").evaluate((host, names) => {
    const probe = document.createElement("span");
    host.append(probe);
    const colors = Object.fromEntries(
      names.map((name) => {
        probe.style.color = `var(--doc-syntax-${name})`;
        return [name, getComputedStyle(probe).color];
      }),
    );
    probe.remove();
    return colors;
  }, tokens);
}

const exactly = (text: string) =>
  new RegExp(`^${text.replace(/[.*+?^$()|[\]{}\\/]/g, "\\$&")}$`);

/** How the first highlighted span with exactly this text is painted. */
async function painted(page: Page, text: string): Promise<Painted> {
  const span = page
    .locator(".file-surface .cm-line span[class]")
    .filter({ hasText: exactly(text) })
    .first();
  await expect(span).toBeVisible();
  return span.evaluate((node) => {
    const style = getComputedStyle(node);
    return {
      color: style.color,
      fontStyle: style.fontStyle,
      fontWeight: style.fontWeight,
      textDecorationLine: style.textDecorationLine,
    };
  });
}

test.describe("Editor syntax highlighting", () => {
  test.use({ viewport: { width: 1380, height: 880 } });

  test("colours representative TypeScript from the document syntax tokens", async ({
    page,
  }) => {
    await openScenario(page, "editor", "editor");
    await replaceDocument(
      page,
      "Edit src/workbench/AgentChatPanes.tsx",
      TYPESCRIPT,
    );
    const samples: [string, SyntaxToken][] = [
      ["// note", "comment"],
      ["class", "keyword"],
      ["Box", "type"],
      ["size", "variable-definition"],
      ["2", "literal"],
      ['"text"', "string"],
      ["/ab+c/", "regexp"],
      ["true", "atom"],
      ["@", "meta"],
    ];
    const colors = await tokenColors(
      page,
      samples.map(([, token]) => token),
    );
    // The samples only prove the mapping if the tokens tell them apart.
    expect(
      new Set(
        (
          [
            "comment",
            "keyword",
            "type",
            "variable-definition",
            "literal",
            "string",
          ] as const
        ).map((token) => colors[token]),
      ).size,
    ).toBe(6);
    for (const [text, token] of samples) {
      expect((await painted(page, text)).color, `${text} as ${token}`).toBe(
        colors[token],
      );
    }
  });

  test("keeps the default Markdown treatments in token colours", async ({
    page,
  }) => {
    await openScenario(page, "editor", "editor");
    await page.evaluate(async () => {
      const { useWorkbenchStore } = await import("/src/app/store.ts");
      await useWorkbenchStore
        .getState()
        .openFile({ rootId: "visual-root", relativePath: "README.md" });
    });
    await replaceDocument(page, "Edit README.md", MARKDOWN);
    const colors = await tokenColors(page, ["meta", "atom"]);

    expect(await painted(page, " Title")).toMatchObject({
      fontWeight: "700",
      textDecorationLine: "underline",
    });
    expect(await painted(page, "#")).toMatchObject({ color: colors.meta });
    expect(await painted(page, "soft")).toMatchObject({ fontStyle: "italic" });
    expect(await painted(page, "loud")).toMatchObject({ fontWeight: "700" });
    expect(await painted(page, "a link")).toMatchObject({
      textDecorationLine: "underline",
    });
    expect(await painted(page, "https://example.com")).toMatchObject({
      color: colors.atom,
      textDecorationLine: "underline",
    });
  });
});
