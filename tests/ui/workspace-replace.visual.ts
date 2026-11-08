import { expect, test, type Page } from "@playwright/test";

import {
  expectNoViewportOverflow,
  openScenario,
} from "./shared/visualBrowserPrimitives";

// Workspace Replace from the Search panel: a regex Replace all over a File
// with unsaved edits (changed in its editor, where Undo takes it back) and a
// File on disk (changed natively, here by the fixture gateway).
const SOURCE = [
  "const total = count(items);",
  "const label = count(names);",
  "const width = count(columns);",
  "",
].join("\n");

const OPEN_FILE = "src/workbench/AgentChatPanes.tsx";
const DISK_FILE = "src/workbench/sizes.ts";

async function installReplaceFixtures(page: Page) {
  await page.evaluate(
    async ({ openFile, diskFile, source }) => {
      const search =
        await import("/src/features/search/workspaceSearchDesktop.ts");
      const replace = await import("/src/features/search/workspaceReplace.ts");
      const rootId = "visual-root";
      const lines = source.split("\n");
      const matches = [
        ...lines.slice(0, 3).map((preview, index) => ({
          resource: { rootId, relativePath: openFile },
          lineNumber: index + 1,
          columnNumber: 15,
          preview,
        })),
        {
          resource: { rootId, relativePath: diskFile },
          lineNumber: 4,
          columnNumber: 8,
          preview: "return count(rows);",
        },
      ];
      search.workspaceSearchDesktop.search = async (request) => ({
        requestId: "visual-replace",
        projectId: request.projectId,
        transitionEpoch: request.expectedTransitionEpoch,
        indexGeneration: request.indexGeneration,
        query: request.query,
        matches,
        roots: [
          {
            rootId,
            rootName: "Sage",
            state: "complete",
            filesScanned: 2,
            bytesScanned: 200,
            entriesVisited: 2,
            skippedFiles: 0,
            errorCount: 0,
            message: null,
          },
        ],
        truncated: false,
        continuationToken: null,
        limitReasons: [],
        elapsedMs: 3,
      });
      const calls: string[] = [];
      Object.assign(window, { replaceCalls: calls });
      const gateway = replace.workspaceReplaceDesktop;
      // The native engine's line-by-line Replace, for the unsaved text.
      gateway.replaceText = async ({ spec, text }) => {
        calls.push(`text:${spec.useRegex}`);
        const pattern = new RegExp(spec.query, "g");
        let count = 0;
        const changed: {
          lineNumber: number;
          columnNumber: number;
          before: string;
          after: string;
        }[] = [];
        const content = text
          .split("\n")
          .map((line, index) => {
            const found = [...line.matchAll(pattern)];
            if (!found.length) return line;
            count += found.length;
            const after = line.replace(pattern, spec.replacement);
            changed.push({
              lineNumber: index + 1,
              columnNumber: (found[0].index ?? 0) + 1,
              before: line,
              after,
            });
            return after;
          })
          .join("\n");
        return { content, count, lines: changed, linesTruncated: false };
      };
      gateway.preview = async ({ resources }) => {
        calls.push(`preview:${resources.map((r) => r.relativePath).join()}`);
        return {
          files: resources.map((resource) => ({
            resource,
            fingerprint: "visual-disk-v1",
            count: 1,
            lines: [
              {
                lineNumber: 4,
                columnNumber: 8,
                before: "return count(rows);",
                after: "return size(rows);",
              },
            ],
            linesTruncated: false,
          })),
          problems: [],
          truncated: false,
        };
      };
      gateway.apply = async ({ files }) => {
        calls.push(
          `apply:${files.map((f) => `${f.resource.relativePath}@${f.fingerprint}`).join()}`,
        );
        return {
          replaced: files.map((file) => ({
            resource: file.resource,
            count: 1,
            fingerprint: "visual-disk-v2",
          })),
          problems: [],
        };
      };
    },
    { openFile: OPEN_FILE, diskFile: DISK_FILE, source: SOURCE },
  );
}

const line = (page: Page, index: number) => page.locator(".cm-line").nth(index);

test.describe("Workspace Replace", () => {
  test("previews a regex Replace all, changes unsaved edits in the editor, and undoes them", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1380, height: 880 });
    await openScenario(page, "editor", "editor");
    const content = page.locator(".cm-content");
    await content.click();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.insertText(SOURCE);
    await expect(line(page, 0)).toHaveText("const total = count(items);");
    await installReplaceFixtures(page);

    await page.getByRole("button", { name: "Search", exact: true }).click();
    const searchTools = page.getByLabel("Search project tools", {
      exact: true,
    });
    await searchTools
      .getByRole("button", { name: "Use regular expression" })
      .click();
    await searchTools.getByRole("searchbox").fill("count\\((\\w+)\\)");
    await searchTools.getByRole("searchbox").press("Enter");
    await expect(
      searchTools.getByRole("button", {
        name: `Open ${DISK_FILE} at line 4, column 8`,
      }),
    ).toBeVisible();

    await searchTools.getByRole("button", { name: "Toggle Replace" }).click();
    const replacement = searchTools.getByRole("textbox", {
      name: "Replace text",
    });
    await expect(replacement).toHaveAttribute(
      "placeholder",
      "Replace ($1 for a group)",
    );
    await replacement.fill("size($1)");
    await expect(
      searchTools.getByRole("button", { name: `Replace all in ${OPEN_FILE}` }),
    ).toBeVisible();
    await expectNoViewportOverflow(page);
    await searchTools
      .getByRole("button", { name: "Replace all", exact: true })
      .click();

    const dialog = page.getByRole("dialog", {
      name: "Replace 4 matches in 2 files?",
    });
    await expect(dialog).toBeVisible();
    const changes = dialog.getByRole("list", { name: "Changes to make" });
    await expect(
      changes.getByRole("listitem", { name: `${OPEN_FILE}: 3 matches` }),
    ).toContainText("3 matches · unsaved edits");
    await expect(changes.locator("ins").first()).toHaveText(
      "const total = size(items);",
    );
    await expect(
      changes.getByRole("listitem", { name: `${DISK_FILE}: 1 match` }),
    ).toContainText("return size(rows);");
    // Nothing is written before the confirm.
    expect(
      await page.evaluate(
        () => (window as unknown as { replaceCalls: string[] }).replaceCalls,
      ),
    ).toEqual(["text:true", `preview:${DISK_FILE}`]);
    await expect(line(page, 0)).toHaveText("const total = count(items);");

    await dialog.getByRole("button", { name: "Replace", exact: true }).click();
    await expect(dialog).toBeHidden();
    await expect(
      searchTools.getByText("Replaced 4 matches in 2 files."),
    ).toBeVisible();
    expect(
      await page.evaluate(
        () => (window as unknown as { replaceCalls: string[] }).replaceCalls,
      ),
    ).toEqual([
      "text:true",
      `preview:${DISK_FILE}`,
      `apply:${DISK_FILE}@visual-disk-v1`,
    ]);
    // The unsaved File changed in its editor, not on disk.
    await expect(line(page, 0)).toHaveText("const total = size(items);");
    await expect(line(page, 2)).toHaveText("const width = size(columns);");

    // One Undo takes the whole Replace back.
    await content.click();
    await page.keyboard.press("ControlOrMeta+z");
    await expect(line(page, 0)).toHaveText("const total = count(items);");
    await expect(line(page, 1)).toHaveText("const label = count(names);");
    await expect(line(page, 2)).toHaveText("const width = count(columns);");
  });
});
