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

const thumbnailCases = [
  { name: "a 4000x10 PNG", width: 4000, height: 10, rotated: false },
  { name: "a 10x4000 PNG", width: 10, height: 4000, rotated: false },
  { name: "a rotated 2048x16 JPEG", width: 2048, height: 16, rotated: true },
];

for (const { name, width, height, rotated } of thumbnailCases) {
  test(`${name} previews as an upright square thumbnail of its center`, async ({
    page,
  }) => {
    await openScenario(page, "agent-single", "agent");
    const thumbnail = await page.evaluate(
      async ({ width, height, rotated }) => {
        const source = document.createElement("canvas");
        source.width = width;
        source.height = height;
        const draw = source.getContext("2d")!;
        draw.fillStyle = "#ff0000";
        draw.fillRect(0, 0, width, height / 2);
        draw.fillStyle = "#0000ff";
        draw.fillRect(0, height / 2, width, height / 2);
        const encoded = await new Promise<Blob>((resolve) =>
          source.toBlob(
            (value) => resolve(value!),
            rotated ? "image/jpeg" : "image/png",
            1,
          ),
        );
        // An APP1 Exif segment with Orientation 6: shown rotated 90° clockwise.
        const orientation = new Uint8Array([
          0xff, 0xe1, 0x00, 0x22, 0x45, 0x78, 0x69, 0x66, 0x00, 0x00, 0x4d,
          0x4d, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x08, 0x00, 0x01, 0x01, 0x12,
          0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x06, 0x00, 0x00, 0x00,
          0x00, 0x00, 0x00,
        ]);
        const file = rotated
          ? new Blob([encoded.slice(0, 2), orientation, encoded.slice(2)], {
              type: "image/jpeg",
            })
          : encoded;
        const { createImageThumbnail } =
          await import("/src/features/chat/imageThumbnail.ts");
        const decode = window.createImageBitmap;
        let largestBitmap = 0;
        window.createImageBitmap = async (
          ...args: Parameters<typeof createImageBitmap>
        ) => {
          const bitmap = await decode(...args);
          largestBitmap = Math.max(largestBitmap, bitmap.width * bitmap.height);
          return bitmap;
        };
        const preview = await createImageThumbnail(file).finally(
          () => (window.createImageBitmap = decode),
        );
        const bitmap = await createImageBitmap(preview!);
        const canvas = document.createElement("canvas");
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        const read = canvas.getContext("2d")!;
        read.drawImage(bitmap, 0, 0);
        const hue = (x: number, y: number) => {
          const [red, , blue] = read.getImageData(x, y, 1, 1).data;
          return red > blue ? "red" : "blue";
        };
        return {
          width: bitmap.width,
          height: bitmap.height,
          corners: [hue(6, 6), hue(42, 6), hue(6, 42), hue(42, 42)],
          largestBitmap,
        };
      },
      { width, height, rotated },
    );
    expect(thumbnail).toEqual({
      width: IMAGE_THUMBNAIL_EDGE_PX,
      height: IMAGE_THUMBNAIL_EDGE_PX,
      corners: rotated
        ? ["blue", "red", "blue", "red"]
        : ["red", "red", "blue", "blue"],
      largestBitmap: IMAGE_THUMBNAIL_EDGE_PX ** 2,
    });
  });
}

test("an image the WebView cannot decode still attaches, without a preview", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await prepare(page, true);
  await page
    .locator("form.composer")
    .first()
    .getByRole("combobox", { name: "Message agent" })
    .evaluate((textbox) => {
      const transfer = new DataTransfer();
      transfer.items.add(
        new File([new Uint8Array(64)], "broken.png", { type: "image/png" }),
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
  const composer = page.locator("form.composer").first();
  await expect(
    composer.getByRole("button", { name: "Remove Image · broken.png context" }),
  ).toBeVisible();
  await expect(composer.locator(".composer__image-preview")).toHaveCount(0);
  await expect(composer.getByRole("alert")).toHaveCount(0);
});
