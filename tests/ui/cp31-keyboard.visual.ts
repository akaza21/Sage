import { expect, test } from "@playwright/test";
import { expectScrollStop, installDiagrams } from "./cp31DiagramHelpers";
import { openScenario } from "./shared/visualBrowserPrimitives";
import { screenshotChromiumArgs } from "./shared/visualBrowserOptions";

test.use({
  viewport: { width: 1888, height: 1096 },
  deviceScaleFactor: 1,
  launchOptions: {
    args: [...screenshotChromiumArgs, "--disable-smooth-scrolling"],
  },
});

test("CP-31 instant diagram scrolling completes across color modes", async ({
  page,
}) => {
  await openScenario(page, "agent-single", "agent");
  await installDiagrams(
    page,
    `sequenceDiagram\nAlice->>Bob: ${"W".repeat(64)}`,
  );
  const figure = page.getByRole("figure", { name: /^Diagram:/u });
  await expect(figure.getByRole("img")).toBeVisible();
  for (const forcedColors of ["none", "active"] as const) {
    await page.emulateMedia({ forcedColors, reducedMotion: "reduce" });
    await expect
      .poll(() =>
        figure
          .locator(".acp-mermaid")
          .evaluate((element) => element.scrollWidth - element.clientWidth),
      )
      .toBeGreaterThan(1);
    await expectScrollStop(page, figure, true);
  }
});
