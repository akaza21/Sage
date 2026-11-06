import { expect, test } from "./shared/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

for (const width of [1400, 900]) {
  test(`new chat project subtitle is centred under its heading at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 1000 });
    await openScenario(page, "agent-single", "agent");
    await settleChatProjections(page);
    await page.evaluate(async () => {
      const { installChatChromeFixture } = await sageImport(
        "/tests/ui/fixtures/chatChromeFixture.ts",
      );
      const { useWorkbenchStore } = await sageImport("/src/app/store.ts");
      installChatChromeFixture("codex", true);
      const project = useWorkbenchStore.getState().project!;
      useWorkbenchStore.setState({ project: { ...project, name: "Local" } });
    });
    const home = page.getByRole("region", { name: "New chat", exact: true });
    const heading = home.getByRole("heading", {
      name: "What should we work on?",
    });
    const subtitle = home.getByRole("button", { name: "Project: Local" });
    await expect(heading).toBeVisible();
    await expect(subtitle).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    const headingBox = (await heading.boundingBox())!;
    const textBox = await subtitle
      .locator(".searchable-picker__trigger-label")
      .evaluate((element) => {
        const range = document.createRange();
        range.selectNodeContents(element);
        const box = range.getBoundingClientRect();
        return { x: box.x, width: box.width };
      });
    expect(
      Math.abs(
        textBox.x + textBox.width / 2 - (headingBox.x + headingBox.width / 2),
      ),
    ).toBeLessThanOrEqual(1);
  });
}
