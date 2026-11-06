import { expect, test, type Page } from "./shared/test";
import { build } from "vite";
import { flowchartSource } from "./fixtures/cp22Answers";

let script: string;
let styles: string;

test.beforeAll(async () => {
  const result = await build({
    configFile: false,
    logLevel: "silent",
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
    build: {
      write: false,
      minify: false,
      lib: {
        entry: "tests/ui/fixtures/richAnswers.tsx",
        formats: ["iife"],
        name: "RichAnswersFixture",
      },
    },
  });
  const output = (Array.isArray(result) ? result : [result]).flatMap(
    (bundle) => {
      if (!("output" in bundle)) throw new Error("Expected a fixture bundle");
      return bundle.output;
    },
  );
  script = output
    .filter((entry) => entry.type === "chunk")
    .map((entry) => entry.code)
    .join("\n");
  styles = output
    .filter(
      (entry) => entry.type === "asset" && entry.fileName.endsWith(".css"),
    )
    .map((entry) => String(entry.source))
    .join("\n");
});

async function render(page: Page, source: string, options: string[]) {
  await page.evaluate(
    ({ source, options }) => {
      const fixture = window as typeof window & {
        renderRichAnswer(source: string, options: string[]): void;
      };
      fixture.renderRichAnswer(source, options);
    },
    { source, options },
  );
}

for (const width of [1888, 720, 480]) {
  test.describe(`${width}px`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize({ width, height: 1096 });
      await page.setContent(
        `<style>${styles}</style><main id="root" style="width: min(660px, calc(100% - 294px)); margin-left: auto; margin-right: 16px"></main>`,
      );
      await page.addScriptTag({ content: script });
    });
    test("rich answer SVGs fit the column at every intrinsic size", async ({
      page,
    }) => {
      for (const source of [
        "flowchart TD\nA --> B",
        flowchartSource,
        "flowchart LR\nBinding --> Pump\nPump --> SQLite",
        "sequenceDiagram\nProcessingNode->>StorageService: Save",
        `sequenceDiagram\nAlice->>Bob: ${"W".repeat(64)}`,
        "classDiagram\nAnimal <|-- Duck\nAnimal : +int age",
        "stateDiagram-v2\n[*] --> Ready\nReady --> Done: finish",
      ]) {
        await render(page, source, [
          "allow_once",
          "allow_always",
          "reject_once",
        ]);
        const figure = page.getByRole("figure");
        const svg = figure.getByRole("img");
        await expect(svg).toBeVisible();
        const metrics = await figure.evaluate((element) => {
          const svg = element.querySelector("svg")!;
          const box = svg.getBoundingClientRect();
          return {
            width: box.width,
            height: box.height,
            naturalWidth: svg.viewBox.baseVal.width,
            naturalHeight: svg.viewBox.baseVal.height,
            column: element.querySelector(".acp-mermaid")!.clientWidth,
            overflow:
              element.querySelector(".acp-mermaid")!.scrollWidth >
              element.querySelector(".acp-mermaid")!.clientWidth + 1,
            figureOverflow: element.scrollWidth > element.clientWidth + 1,
            figureWidth: element.clientWidth,
            rootWidth: document.getElementById("root")!.clientWidth,
            labelSizes: [...svg.querySelectorAll("text")].map(
              (text) =>
                parseFloat(getComputedStyle(text).fontSize) *
                text.getScreenCTM()!.a,
            ),
            labelSize: parseFloat(
              getComputedStyle(svg.querySelector("text")!).fontSize,
            ),
            fills: [...svg.querySelectorAll("rect, polygon")].map((node) => ({
              fill: getComputedStyle(node).fill,
              stroke: getComputedStyle(node).stroke,
            })),
          };
        });
        if (source === flowchartSource && width >= 720) {
          expect(metrics.overflow, `ref-55 flowchart at ${width}`).toBe(false);
          expect(metrics.width).toBeLessThanOrEqual(metrics.column);
        }
        // Fit the column, but keep at least 75% of natural scale; below that
        // the diagram scrolls inside its own box and never widens the column.
        expect(metrics.width).toBeCloseTo(
          Math.max(
            Math.min(metrics.naturalWidth, metrics.column),
            metrics.naturalWidth * 0.75,
          ),
          1,
        );
        expect(metrics.overflow).toBe(metrics.width > metrics.column + 1);
        expect(metrics.figureOverflow).toBe(false);
        expect(metrics.figureWidth).toBeLessThanOrEqual(metrics.rootWidth);
        expect(metrics.height / metrics.naturalHeight).toBeCloseTo(
          metrics.width / metrics.naturalWidth,
          3,
        );
        for (const size of metrics.labelSizes)
          expect(size).toBeCloseTo(
            (metrics.labelSize * metrics.width) / metrics.naturalWidth,
            2,
          );
        for (const node of metrics.fills) {
          expect(node.fill).toBe("rgb(20, 20, 20)");
          expect(node.stroke).toBe("rgb(42, 42, 42)");
        }
        const copy = page.getByRole("button", { name: "Copy diagram source" });
        await expect(copy).toHaveText("");
        await expect(copy.locator("svg")).toBeVisible();
        await expect(copy).toBeInViewport();
      }
    });
    test("approval buttons stay whole and expose only offered choices", async ({
      page,
    }) => {
      await render(page, flowchartSource, [
        "allow_once",
        "allow_always",
        "reject_once",
      ]);
      const buttons = page
        .getByRole("group", { name: "Permission request" })
        .getByRole("button");
      await expect(buttons).toHaveText(["Deny Esc", "Allow once ⏎", ""]);
      for (const button of await buttons.all()) {
        const metrics = await button.evaluate((element) => {
          const range = document.createRange();
          range.selectNodeContents(element);
          const lines = [...range.getClientRects()];
          const box = element.getBoundingClientRect();
          const card = element
            .closest(".acp-permission-prompt")!
            .getBoundingClientRect();
          // A label and its key cap are separate boxes on one row; count
          // rows of vertically overlapping boxes, not boxes.
          const rows: Array<{ top: number; bottom: number }> = [];
          for (const rect of [...lines].sort((a, b) => a.top - b.top)) {
            const row = rows.at(-1);
            if (row && rect.top < row.bottom - 1)
              row.bottom = Math.max(row.bottom, rect.bottom);
            else rows.push({ top: rect.top, bottom: rect.bottom });
          }
          return {
            lines: rows.length,
            inside: box.left >= card.left && box.right <= card.right,
            cardInsideViewport: card.left >= 0 && card.right <= innerWidth,
            labelInside: lines.every(
              (line) =>
                line.left >= box.left &&
                line.right <= box.right &&
                line.top >= box.top &&
                line.bottom <= box.bottom,
            ),
          };
        });
        expect(metrics.lines).toBe(1);
        expect(metrics.inside).toBe(true);
        expect(metrics.cardInsideViewport).toBe(true);
        expect(metrics.labelInside).toBe(true);
      }
      expect(
        await page.evaluate(() => {
          const pane = document.querySelector(".agent-chat-pane")!;
          return (
            pane.scrollWidth <= pane.clientWidth &&
            document.documentElement.scrollWidth <= innerWidth
          );
        }),
      ).toBe(true);
      await render(page, flowchartSource, ["reject_once"]);
      await expect(buttons).toHaveText(["Deny Esc"]);
      await render(page, flowchartSource, []);
      await expect(buttons).toHaveCount(0);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
    });
  });
}
