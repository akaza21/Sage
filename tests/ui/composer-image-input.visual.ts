import { expect, test, type Page } from "@playwright/test";
import { IMAGE_THUMBNAIL_EDGE_PX } from "../../src/features/chat/imageThumbnail";
import { serveUnderTauriCsp } from "./shared/tauriCsp";
import {
  openScenario,
  settleChatProjections,
} from "./shared/visualBrowserPrimitives";

// Captures land in output/ by default; reviews may point them elsewhere.
const captureDir =
  process.env.SAGE_IMAGE_INPUT_CAPTURE_DIR ??
  "output/playwright/composer-image-input";

async function prepare(page: Page, promptImages: boolean) {
  await serveUnderTauriCsp(page);
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
    await expect(preview).toHaveJSProperty(
      "naturalWidth",
      IMAGE_THUMBNAIL_EDGE_PX,
    );
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

for (const [width, height] of [
  [4000, 10],
  [10, 4000],
]) {
  test(`a ${width}x${height} image previews its center as a square thumbnail`, async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const thumbnail = await page.evaluate(
      async ([width, height]) => {
        const source = document.createElement("canvas");
        source.width = width;
        source.height = height;
        const draw = source.getContext("2d")!;
        const along = width > height ? width : height;
        for (const [index, color] of [
          "#ff0000",
          "#00ff00",
          "#0000ff",
        ].entries()) {
          draw.fillStyle = color;
          const start = (along / 3) * index;
          if (width > height) draw.fillRect(start, 0, along / 3, height);
          else draw.fillRect(0, start, width, along / 3);
        }
        const file = await new Promise<Blob>((resolve) =>
          source.toBlob((value) => resolve(value!), "image/png"),
        );
        const { createImageThumbnail } =
          await import("/src/features/chat/imageThumbnail.ts");
        const decode = window.createImageBitmap;
        let largestDecode = 0;
        window.createImageBitmap = async (
          ...args: Parameters<typeof createImageBitmap>
        ) => {
          const bitmap = await decode(...args);
          largestDecode = Math.max(largestDecode, bitmap.width * bitmap.height);
          return bitmap;
        };
        const preview = await createImageThumbnail(file, {
          width,
          height,
        }).finally(() => (window.createImageBitmap = decode));
        const bitmap = await createImageBitmap(preview!);
        const canvas = document.createElement("canvas");
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        const read = canvas.getContext("2d")!;
        read.drawImage(bitmap, 0, 0);
        return {
          width: bitmap.width,
          height: bitmap.height,
          center: Array.from(read.getImageData(24, 24, 1, 1).data),
          largestDecode,
        };
      },
      [width, height],
    );
    expect(thumbnail).toEqual({
      width: IMAGE_THUMBNAIL_EDGE_PX,
      height: IMAGE_THUMBNAIL_EDGE_PX,
      center: [0, 255, 0, 255],
      largestDecode: expect.any(Number),
    });
    expect(thumbnail.largestDecode).toBeLessThanOrEqual(
      (IMAGE_THUMBNAIL_EDGE_PX * 8) ** 2,
    );
  });
}
