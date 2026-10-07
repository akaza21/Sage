import { chromium } from "@playwright/test";

const cdpPort = Number(process.argv[2] ?? 9343);
if (!Number.isInteger(cdpPort) || cdpPort < 1024 || cdpPort > 65535) {
  throw new Error("invalid CDP port");
}

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
try {
  const context = browser.contexts()[0];
  const main = context
    .pages()
    .find((page) => page.url().startsWith("http://127.0.0.1:1430/"));
  if (!main) throw new Error("isolated Sage main WebView not found");
  await main.setViewportSize({ width: 720, height: 560 });
  if (
    (await main.locator(".center-region").getAttribute("aria-hidden")) ===
    "true"
  ) {
    await main.getByRole("button", { name: "Hide left sidebar" }).click();
  }
  const surface = main.getByRole("region", { name: "Browser page" });
  await main.getByRole("button", { name: "New browser tab" }).click();
  await surface.getByText("HTTPS · example.com").waitFor({ timeout: 30000 });
  const child = context
    .pages()
    .find((page) => page.url().startsWith("https://example.com/"));

  if (
    (await surface.getByRole("textbox", { name: "Find text" }).count()) === 0
  ) {
    await surface.getByRole("button", { name: "Find in page" }).click();
  }
  await surface
    .getByRole("textbox", { name: "Find text" })
    .fill("Example Domain");
  await surface.getByRole("button", { name: /^Find$/ }).click();
  await surface.getByRole("button", { name: "Next match" }).click();
  if (await surface.getByRole("alert").count()) {
    throw new Error(
      `native find failed: ${await surface.getByRole("alert").innerText()}`,
    );
  }

  const widthAt100 = child
    ? await child.evaluate(
        () => globalThis.document.documentElement.clientWidth,
      )
    : null;
  let currentZoom = Number(
    (await surface.locator(".browser-surface__zoom span").innerText()).replace(
      "%",
      "",
    ),
  );
  for (; currentZoom < 200; currentZoom += 25) {
    await surface.getByRole("button", { name: "Zoom in" }).click();
    try {
      await surface
        .getByText(`${currentZoom + 25}%`)
        .waitFor({ timeout: 5000 });
    } catch (error) {
      const alert = await surface.getByRole("alert").allInnerTexts();
      const zoom = await surface.locator(".browser-surface__zoom").innerText();
      throw new Error(
        `zoom step ${currentZoom} failed: ${JSON.stringify({ alert, zoom })}`,
        { cause: error },
      );
    }
  }
  const widthAt200 = child
    ? await child.evaluate(
        () => globalThis.document.documentElement.clientWidth,
      )
    : null;
  if (
    widthAt100 !== null &&
    widthAt200 !== null &&
    !(widthAt200 < widthAt100)
  ) {
    throw new Error(
      `native zoom did not shrink CSS viewport: ${widthAt100} -> ${widthAt200}`,
    );
  }

  await main.setViewportSize({ width: 720, height: 560 });
  const layout = await surface.evaluate((element) => {
    const toolbar = element.querySelector(".browser-surface__toolbar");
    const frame = element.querySelector(".browser-surface__frame");
    const controls = [
      ...element.querySelectorAll(".browser-surface__toolbar button"),
    ];
    if (!toolbar || !frame) return null;
    const toolbarBounds = toolbar.getBoundingClientRect();
    const frameBounds = frame.getBoundingClientRect();
    return {
      toolbarBottom: toolbarBounds.bottom,
      frameTop: frameBounds.top,
      frameHeight: frameBounds.height,
      controlsVisible: controls.every((control) => {
        const bounds = control.getBoundingClientRect();
        return (
          bounds.width >= 1 &&
          bounds.height >= 1 &&
          bounds.left >= 0 &&
          bounds.right <= globalThis.innerWidth
        );
      }),
    };
  });
  if (
    !layout ||
    !layout.controlsVisible ||
    layout.frameTop < layout.toolbarBottom ||
    layout.frameHeight < 1
  ) {
    throw new Error(
      `narrow controls are not usable: ${JSON.stringify(layout)}`,
    );
  }
  process.stdout.write(
    JSON.stringify({
      mainUrl: main.url(),
      childUrl: child?.url() ?? "CDP target unavailable (isolated profile)",
      widthAt100,
      widthAt200,
      narrowLayout: layout,
    }) + "\n",
  );
} finally {
  await browser.close();
}
