import { expect, test, type Page } from "@playwright/test";
import os from "node:os";
import path from "node:path";
import { build } from "vite";

// Build outside the repository so the suite dev server never sees the
// fixture HTML change and full-reloads pages owned by other workers.
const output = path.join(
  os.tmpdir(),
  `sage-activity-geometry-${process.env.SAGE_UI_PORT ?? "default"}-${process.pid}`,
);

test.beforeAll(async () => {
  await build({
    configFile: false,
    publicDir: false,
    root: path.resolve("tests/ui/fixtures"),
    build: {
      outDir: output,
      emptyOutDir: true,
      rolldownOptions: {
        input: path.resolve("tests/ui/fixtures/activity-parity.html"),
      },
    },
  });
});

async function open(page: Page, scenario: string) {
  await page.route("https://activity.test/**", async (route) => {
    const pathname = new URL(route.request().url()).pathname;
    const file = path.resolve(
      output,
      `.${pathname === "/" ? "/activity-parity.html" : pathname}`,
    );
    if (!file.startsWith(output + path.sep))
      throw new Error("Escaping fixture request");
    await route.fulfill({ path: file });
  });
  await page.goto(`https://activity.test/?scenario=${scenario}`);
  await expect(page.locator(".acp-turn")).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
}

for (const width of [1888, 720, 480]) {
  test.describe(`${width}px shared transcript geometry`, () => {
    test.use({ viewport: { width, height: 1048 } });

    test("running command stays one line as a shimmering label and no early truncation", async ({
      page,
    }) => {
      await open(page, "running");
      const summary = page.locator(".acp-tool-rollup > summary");
      await expect(summary).toHaveText(
        "Running git diff --stat -- src/features/chat/acp/ToolActivityRollupView.tsx",
      );
      await expect(summary.locator(".acp-tool-running")).toHaveCount(0);
      await expect(summary.locator(".acp-live-shimmer")).toHaveCount(1);
      await expect(
        summary.locator(".acp-tool-rollup__summary-copy svg"),
      ).toHaveCount(1);
      // Codex refs 08 and 20 keep the running row on one line and clip it only
      // at the column edge; nothing trailing steals the label's width.
      const geometry = await summary
        .locator(".acp-tool-rollup__label")
        .evaluate((label) => {
          const style = getComputedStyle(label);
          const probe = label.cloneNode() as HTMLElement;
          probe.textContent = "x";
          label.after(probe);
          const line = probe.getBoundingClientRect().height;
          probe.remove();
          const box = label.getBoundingClientRect();
          const row = label.closest("summary")!.getBoundingClientRect();
          return {
            clipped: label.scrollWidth > label.clientWidth,
            ellipsis: style.textOverflow,
            oneLine: box.height <= line + 1,
            gapToEdge: row.right - box.right,
            right: box.right,
          };
        });
      expect(geometry.oneLine).toBe(true);
      expect(geometry.ellipsis).toBe("ellipsis");
      // Only the trailing disclosure-chevron slot (<=32px) may follow it.
      if (geometry.clipped) expect(geometry.gapToEdge).toBeLessThanOrEqual(32);
      expect(geometry.right).toBeLessThanOrEqual(width);
    });

    test("Worked reveals per-command Ran rows and opens only the chosen Shell card", async ({
      page,
    }) => {
      await open(page, "commands");
      await page.locator(".chat-transcript-work > summary").click();
      const commands = page.locator(
        ".acp-tool-rollup__items > .acp-tool-rollup",
      );
      await expect(commands).toHaveCount(3);
      for (const command of await commands.all()) {
        await expect(command).not.toHaveAttribute("open");
        await expect(command.locator(":scope > summary")).toContainText(
          "Ran git status --short",
        );
      }
      const cards = page.locator(".acp-tool-command-frame");
      await expect(cards).toHaveCount(0);
      await commands.last().locator(":scope > summary").click();
      await expect(cards).toHaveCount(1);
      for (const card of await cards.all()) await expect(card).toBeVisible();
      await expect(page.locator(".acp-tool-rollup__items header")).toHaveCount(
        0,
      );
      const group = page.locator(
        ".chat-transcript-work__groups > .acp-tool-rollup",
      );
      const row = await group.boundingBox();
      const first = await cards.first().boundingBox();
      const answer = await page
        .locator(".acp-turn__items > .acp-message--assistant")
        .boundingBox();
      expect(Math.abs(first!.x - row!.x)).toBeLessThanOrEqual(1);
      // Codex s5/f0405: an open child stays inside the group's nested
      // scroll area (max 220px), so the card may be clipped by it.
      const scroller = group.locator(":scope > .acp-tool-rollup__items");
      expect(
        await scroller.evaluate((node) => node.clientHeight),
      ).toBeLessThanOrEqual(220);
      // The answer keeps one item gap (~11px measured on text boxes) below
      // the group, not the former ~45px detached spacing.
      expect(answer!.y - (row!.y + row!.height)).toBeGreaterThanOrEqual(8);
      expect(answer!.y - (row!.y + row!.height)).toBeLessThanOrEqual(16);
    });

    test("grouped reads read Read files and open one nested row per file", async ({
      page,
    }) => {
      await open(page, "reads");
      await page.locator(".chat-transcript-work > summary").click();
      await expect(
        page.locator(
          ".chat-transcript-work__groups > .acp-tool-rollup > summary .acp-tool-rollup__label",
        ),
      ).toHaveText("Read files");
      const rows = page.locator(".acp-tool-rollup__items > .acp-tool-rollup");
      await expect(rows).toHaveCount(3);
      await expect(page.locator(".acp-tool-command-frame")).toHaveCount(0);
      await rows.first().locator(":scope > summary").click();
      await expect(page.locator(".acp-tool-command-frame")).toHaveCount(1);
      await expect(page.locator(".acp-tool-rollup__items header")).toHaveCount(
        0,
      );
    });

    test("stopped notice follows the partial answer", async ({ page }) => {
      await open(page, "stopped");
      const answer = await page
        .getByText("Normalized answer", { exact: true })
        .boundingBox();
      const notice = await page
        .getByRole("status", { name: "Cancelled by you" })
        .boundingBox();
      expect(notice!.y).toBeGreaterThanOrEqual(answer!.y + answer!.height);
      await expect(page.getByText("Partial answer retained")).toHaveCount(0);
    });

    test("Mermaid keeps a legible scale and is a keyboard scroll stop only when it overflows", async ({
      page,
    }) => {
      await open(page, "mermaid");
      const figure = page.locator(".acp-mermaid");
      await expect(figure).toHaveAttribute("role", "region");
      // No forced tabindex: the browser focuses the scroller only on overflow.
      await expect(figure).not.toHaveAttribute("tabindex");
      await expect(figure).toHaveAccessibleName(/Diagram:.*scroll area/);
      await expect(figure.locator("svg")).toBeVisible();
      const geometry = await figure.evaluate((element) => {
        const svg = element.querySelector("svg")!;
        const box = svg.getBoundingClientRect();
        const region = element.getBoundingClientRect();
        const column = element
          .closest(".acp-turn__items")!
          .getBoundingClientRect();
        return {
          svg: { left: box.left, right: box.right, width: box.width },
          region: { left: region.left, right: region.right },
          minimumWidth: Number(svg.getAttribute("width")) * 0.75,
          availableWidth: element.clientWidth,
          column: {
            left: column.left,
            right: column.right,
            width: column.width,
          },
          overflow: element.scrollWidth > element.clientWidth,
          nodesInside: [...svg.querySelectorAll("g[transform]")].every(
            (node) => {
              const nodeBox = node.getBoundingClientRect();
              return (
                nodeBox.left >= box.left - 1 && nodeBox.right <= box.right + 1
              );
            },
          ),
        };
      });
      expect(geometry.svg.width).toBeGreaterThan(0);
      expect(geometry.svg.width).toBeGreaterThanOrEqual(
        geometry.minimumWidth - 1,
      );
      expect(geometry.region.left).toBeGreaterThanOrEqual(geometry.column.left);
      expect(geometry.region.right).toBeLessThanOrEqual(
        geometry.column.right + 1,
      );
      expect(geometry.overflow).toBe(
        geometry.minimumWidth > geometry.availableWidth + 1,
      );
      expect(geometry.nodesInside).toBe(true);
      expect(
        await figure.evaluate((element) => {
          (element as HTMLElement).focus();
          return document.activeElement === element;
        }),
      ).toBe(geometry.overflow);
      await expect(
        page.getByRole("button", { name: "Copy diagram source" }),
      ).toHaveCount(0);
      await expect(
        page.getByRole("button", { name: "Copy", exact: true }),
      ).toHaveCount(1);
    });

    test("thinking after prose has the reference gap", async ({ page }) => {
      await open(page, "thinking");
      const prose = await page
        .getByText("Checking the project now.", { exact: true })
        .boundingBox();
      const thinking = await page.locator(".acp-thought").boundingBox();
      expect(thinking!.y - (prose!.y + prose!.height)).toBeGreaterThanOrEqual(
        18,
      );
      expect(thinking!.y - (prose!.y + prose!.height)).toBeLessThanOrEqual(24);
    });

    test("failed compaction uses an icon-led normal row and error tone", async ({
      page,
    }) => {
      await open(page, "compaction");
      await page.locator(".chat-transcript-work > summary").click();
      const row = page.locator(".acp-compaction-notice");
      await expect(row.locator(":scope > svg")).toHaveCount(1);
      expect((await row.boundingBox())!.height).toBeGreaterThanOrEqual(28);
      expect(
        await row.evaluate((element) => getComputedStyle(element).color),
      ).toBe(
        await row.evaluate((element) => {
          const probe = document.createElement("span");
          probe.style.color = "var(--color-danger)";
          element.append(probe);
          const color = getComputedStyle(probe).color;
          probe.remove();
          return color;
        }),
      );
    });

    test("named subagent avatars remain distinct beside the group names", async ({
      page,
    }) => {
      await open(page, "subagents");
      await page.locator(".chat-transcript-work > summary").click();
      await expect(
        page.getByText("Reviewer and Builder finished", { exact: true }),
      ).toBeVisible();
      const avatars = await page
        .locator(".acp-subagent-row__glyph")
        .evaluateAll((elements) =>
          elements.map((element) => {
            const box = element.getBoundingClientRect();
            return { left: box.left, right: box.right, width: box.width };
          }),
        );
      expect(avatars).toHaveLength(2);
      // Codex s4/f0485 (parity spec section 7): 14 px glyphs, one per child.
      expect(avatars[0].width).toBe(14);
      expect(avatars[1].width).toBe(14);
      expect(avatars[1].left - avatars[0].right).toBeGreaterThanOrEqual(4);
    });

    test("simple answers keep Worked; tool-only turns have no action row", async ({
      page,
    }) => {
      await open(page, "simple");
      await expect(
        page.getByText("Worked for 5s", { exact: true }),
      ).toBeVisible();
      await expect(page.locator("details.chat-transcript-work")).toHaveCount(0);
      await page.goto("https://activity.test/?scenario=tool-only");
      await expect(page.locator(".acp-turn")).toBeVisible();
      await expect(page.locator(".chat-transcript-actions")).toHaveCount(0);
    });
  });
}
