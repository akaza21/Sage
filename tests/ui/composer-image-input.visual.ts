import { expect, test, type Page } from "@playwright/test";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

// Captures land in output/ by default; reviews may point them elsewhere.
const captureDir =
  process.env.SAGE_IMAGE_INPUT_CAPTURE_DIR ??
  "output/playwright/composer-image-input";

async function prepare(page: Page, promptImages: boolean) {
  await openScenario(page, "agent-single", "agent");
  await settleChatProjections(page);
  await page.evaluate(async (images) => {
    const fixture = await import("/tests/ui/fixtures/composerImageFixture.ts");
    fixture.installComposerImageFixture(images);
  }, promptImages);
}

/** Pastes a generated PNG screenshot into the composer, as a clipboard would. */
async function pasteScreenshot(page: Page) {
  await page
    .locator("form.composer")
    .first()
    .getByRole("combobox", { name: "Message agent" })
    .evaluate(async (textbox) => {
      const canvas = document.createElement("canvas");
      canvas.width = 320;
      canvas.height = 200;
      const context = canvas.getContext("2d")!;
      context.fillStyle = "#1f2937";
      context.fillRect(0, 0, 320, 200);
      context.fillStyle = "#60a5fa";
      context.fillRect(16, 16, 288, 28);
      context.fillStyle = "#e5e7eb";
      for (let row = 0; row < 5; row += 1)
        context.fillRect(16, 60 + row * 24, 120 + row * 30, 12);
      const blob = await new Promise<Blob>((resolve) =>
        canvas.toBlob((value) => resolve(value!), "image/png"),
      );
      const transfer = new DataTransfer();
      transfer.items.add(
        new File([blob], "screenshot.png", { type: "image/png" }),
      );
      textbox.focus();
      textbox.dispatchEvent(
        new ClipboardEvent("paste", {
          clipboardData: transfer,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
}

for (const width of [1280, 720]) {
  test(`Claude composer shows a pasted image at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await prepare(page, true);
    await pasteScreenshot(page);
    const composer = page.locator("form.composer").first();
    const preview = composer.locator(".composer__image-preview");
    await expect(preview).toBeVisible();
    await expect(preview).toHaveJSProperty("complete", true);
    await expect(composer.getByRole("alert")).toHaveCount(0);
    await page.screenshot({
      path: `${captureDir}/claude-composer-pasted-image-${width}.png`,
    });
  });
}

test("a session without the image capability refuses a paste with its reason", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await prepare(page, false);
  await pasteScreenshot(page);
  const composer = page.locator("form.composer").first();
  await expect(composer.getByRole("alert")).toHaveText(
    "This harness does not advertise image input.",
  );
  await expect(composer.locator(".composer__image-preview")).toHaveCount(0);
});
