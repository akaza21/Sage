import { expect, test, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";
import { flowchartSource } from "./fixtures/cp22Answers";

const diagrams = [
  ["wide flowchart", "flowchart LR\nBinding --> Pump\nPump --> SQLite"],
  ["ref 55 flowchart", flowchartSource],
  ["long flowchart edge", `graph TD\nA -->|${"W".repeat(64)}| B`],
  [
    "long class member",
    `classDiagram\nExample : +${"W".repeat(63)}\nExample --> Other`,
  ],
  ["long sequence message", `sequenceDiagram\nAlice->>Bob: ${"W".repeat(64)}`],
] as const;

async function installDiagrams(page: Page, source: string) {
  await settleChatProjections(page);
  await page.evaluate(async (source) => {
    const [{ acpProjectionStoreFor }, ids] = await Promise.all([
      import("/src/app/acpProjectionRegistry.ts"),
      import("/tests/ui/fixtures/visualFixtureIds.ts"),
    ]);
    for (const projection of document.querySelectorAll<HTMLElement>(
      ".acp-chat-projection[data-chat-id]",
    )) {
      const chatId = projection.dataset.chatId;
      if (!chatId) throw new Error("Chat pane has no owner");
      const store = acpProjectionStoreFor(chatId);
      const current = structuredClone(store.getState().page);
      const turn = current?.turns.at(-1);
      if (!current || !turn) throw new Error("Expected settled chat turn");
      current.projectionRevision = String(
        BigInt(current.projectionRevision) + 1n,
      ) as typeof current.projectionRevision;
      const prompt = turn.items.find(
        (item) => item.kind === "text" && item.payload.role === "user",
      );
      if (!prompt || prompt.kind !== "text") throw new Error("Expected prompt");
      const answer = {
        ...prompt,
        id: `cp31-answer-${chatId}` as typeof prompt.id,
        ordinal: 2,
        lifecycle: "completed" as const,
        payload: {
          role: "assistant" as const,
          text: `\`\`\`mermaid\n${source}\n\`\`\``,
        },
      };
      turn.items = [prompt, answer];
      turn.activities = [];
      turn.state = "completed";
      turn.terminalAt = turn.updatedAt;
      store.getState().applyAuthoritativePage(
        {
          projectId: ids.PROJECT_ID,
          projectGeneration: ids.GENERATION,
          chatId: current.chatId,
        },
        current,
      );
    }
  }, source);
}

test.use({ deviceScaleFactor: 1 });

for (const width of [480, 720, 1888]) {
  for (const direction of ["TD", "LR"]) {
    for (const length of [28, 64]) {
      for (const [shape, open, close] of [
        ["rect", "[", "]"],
        ["round", "(", ")"],
        ["diamond", "{", "}"],
      ]) {
        test(`CP-31 long sibling labels have disjoint bounded ${shape} node boxes at ${width}px (${direction}, ${length} characters)`, async ({
          page,
        }) => {
          const label = "W".repeat(length);
          await page.setViewportSize({ width, height: 1096 });
          await openScenario(page, "agent-single", "agent");
          await installDiagrams(
            page,
            `graph ${direction}\nStart --> Left${open}${label}${close}\nStart --> Right${open}${label}${close}\nRight --> End`,
          );
          const figure = page.getByRole("figure", { name: "Diagram" });
          await expect(figure.getByRole("img")).toBeVisible();
          await page.evaluate(() => document.fonts.ready);
          const metrics = await figure.evaluate((element) => {
            const svg = element.querySelector("svg")!;
            const svgBox = svg.getBoundingClientRect();
            const bodySize = parseFloat(
              getComputedStyle(element.closest(".acp-markdown")!).fontSize,
            );
            return {
              bodySize,
              nodes: [...svg.querySelectorAll<SVGGElement>(":scope > g")]
                .filter((group) => group.querySelector("rect, polygon"))
                .map((group) => {
                  const box = group.getBoundingClientRect();
                  const shape = group
                    .querySelector("rect, polygon")!
                    .getBoundingClientRect();
                  const text = group.querySelector("text")!;
                  const labelBox = text.getBoundingClientRect();
                  return {
                    left: box.left,
                    right: box.right,
                    top: box.top,
                    bottom: box.bottom,
                    width: box.width,
                    labelSize:
                      parseFloat(getComputedStyle(text).fontSize) *
                      text.getScreenCTM()!.a,
                    labelInsideNode:
                      labelBox.left >= shape.left &&
                      labelBox.right <= shape.right &&
                      labelBox.top >= shape.top &&
                      labelBox.bottom <= shape.bottom,
                    insideSvg:
                      box.left >= svgBox.left &&
                      box.right <= svgBox.right &&
                      box.top >= svgBox.top &&
                      box.bottom <= svgBox.bottom,
                    lines: [...text.querySelectorAll("tspan")].map(
                      (line) => line.textContent,
                    ),
                  };
                }),
              intrinsicWidth: svg.viewBox.baseVal.width,
              renderedWidth: svgBox.width,
              hasLocalOverflow: element.scrollWidth > element.clientWidth,
              clientWidth: element.clientWidth,
              paneContained:
                element.closest(".agent-chat-pane")!.scrollWidth <=
                element.closest(".agent-chat-pane")!.clientWidth + 1,
            };
          });
          expect(metrics.nodes).toHaveLength(4);
          for (const [index, node] of metrics.nodes.entries()) {
            for (const other of metrics.nodes.slice(index + 1)) {
              expect(
                node.right <= other.left ||
                  other.right <= node.left ||
                  node.bottom <= other.top ||
                  other.bottom <= node.top,
              ).toBe(true);
            }
            expect(node.width).toBeLessThanOrEqual(320);
            expect(node.labelInsideNode).toBe(true);
            expect(node.insideSvg).toBe(true);
            expect(node.labelSize).toBeCloseTo(metrics.bodySize, 1);
          }
          for (const node of metrics.nodes.slice(1, 3)) {
            expect(node.lines.length).toBeGreaterThan(1);
            expect(node.lines.join("")).toBe(label);
          }
          expect(metrics.renderedWidth).toBeCloseTo(metrics.intrinsicWidth, 1);
          expect(metrics.hasLocalOverflow).toBe(
            metrics.intrinsicWidth > metrics.clientWidth,
          );
          expect(metrics.paneContained).toBe(true);
          expect(
            await page.evaluate(
              () => document.documentElement.scrollWidth <= innerWidth,
            ),
          ).toBe(true);
        });
      }
    }
  }
}

for (const width of [720, 1888]) {
  for (const scenario of ["agent-single", "agent-dual"] as const) {
    for (const [name, source] of diagrams) {
      test(`CP-31 ${name} keeps readable labels inside the SVG at ${width}px in ${scenario}`, async ({
        page,
      }) => {
        await page.setViewportSize({ width, height: 1096 });
        await openScenario(page, scenario, "agent");
        await installDiagrams(page, source);
        const panes = page.locator(".agent-chat-pane");
        await expect(panes).toHaveCount(
          scenario === "agent-dual" && width === 1888 ? 2 : 1,
        );
        for (const forcedColors of ["none", "active"] as const) {
          await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
          for (const pane of await panes.all()) {
            const figure = pane.getByRole("figure", { name: "Diagram" });
            await expect(figure.getByRole("img")).toBeVisible();
            const metrics = await figure.evaluate((element) => {
              const svg = element.querySelector("svg")!;
              const labels = [...svg.querySelectorAll("text")];
              const bodySize = parseFloat(
                getComputedStyle(element.closest(".acp-markdown")!).fontSize,
              );
              const style = getComputedStyle(element);
              return {
                bodySize,
                labelSizes: labels.map(
                  (label) =>
                    parseFloat(getComputedStyle(label).fontSize) *
                    label.getScreenCTM()!.a,
                ),
                intrinsicWidth: svg.viewBox.baseVal.width,
                renderedWidth: svg.getBoundingClientRect().width,
                labelsInsideSvg: labels.every((label) => {
                  const labelBox = label.getBoundingClientRect();
                  const svgBox = svg.getBoundingClientRect();
                  return (
                    labelBox.left >= svgBox.left - 1 &&
                    labelBox.right <= svgBox.right + 1 &&
                    labelBox.top >= svgBox.top - 1 &&
                    labelBox.bottom <= svgBox.bottom + 1
                  );
                }),
                clientWidth: element.clientWidth,
                hasLocalOverflow: element.scrollWidth > element.clientWidth,
                paneContained:
                  element.closest(".agent-chat-pane")!.scrollWidth <=
                  element.closest(".agent-chat-pane")!.clientWidth + 1,
                background: style.backgroundColor,
                border: style.borderTopWidth,
                padding: style.paddingTop,
                nodeFills: [...svg.querySelectorAll("rect")].map(
                  (node) => getComputedStyle(node).fill,
                ),
              };
            });
            expect(metrics.labelSizes.length).toBeGreaterThan(0);
            for (const size of metrics.labelSizes)
              expect(size).toBeCloseTo(metrics.bodySize, 1);
            expect(metrics.renderedWidth).toBeCloseTo(
              metrics.intrinsicWidth,
              1,
            );
            expect(metrics.hasLocalOverflow).toBe(
              metrics.intrinsicWidth > metrics.clientWidth,
            );
            expect(metrics.labelsInsideSvg).toBe(true);
            expect(metrics.paneContained).toBe(true);
            expect(metrics.background).toBe("rgba(0, 0, 0, 0)");
            expect(metrics.border).toBe("0px");
            expect(metrics.padding).toBe("0px");
            for (const fill of metrics.nodeFills) expect(fill).toBe("none");
            if (metrics.hasLocalOverflow) {
              await figure.evaluate((element) => {
                element.scrollLeft = 0;
              });
              await figure.focus();
              await page.keyboard.press("ArrowRight");
              await expect
                .poll(() => figure.evaluate((element) => element.scrollLeft))
                .toBeGreaterThan(0);
            }
          }
        }
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth <= innerWidth,
          ),
        ).toBe(true);
      });
    }
  }
}
