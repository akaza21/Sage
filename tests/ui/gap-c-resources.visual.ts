import { expect, test } from "@playwright/test";
import { forbidBrowserRetries } from "./shared/browserRetryGuard";

forbidBrowserRetries();

test("compact file header and keyboard-scrollable rounded preview fit a narrow pane", async ({
  page,
}) => {
  await page.setViewportSize({ width: 320, height: 500 });
  await page.goto("/tests/ui/gap-c-surfaces.html?surface=file");
  const header = page.locator(".agent-artifact-header");
  const preview = page.getByRole("region", { name: "ts code" });
  await expect(preview).toBeVisible();
  expect((await header.boundingBox())!.height).toBeLessThanOrEqual(40);
  await expect(page.getByText(/2,048 disk bytes/)).not.toBeVisible();
  await page.getByLabel("Resource details").click();
  await expect(page.getByText(/2,048 disk bytes/)).toBeVisible();
  await page.getByLabel("Resource details").click();
  for (const label of ["View source", "Open choices"]) {
    const bounds = (await page
      .getByRole("button", { name: label, exact: true })
      .boundingBox())!;
    expect(bounds.right ?? bounds.x + bounds.width).toBeLessThanOrEqual(320);
  }
  await preview.focus();
  await expect(preview).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect
    .poll(() => preview.evaluate((node) => node.scrollLeft))
    .toBeGreaterThan(0);
  const geometry = await preview.evaluate((node) => ({
    radius: parseFloat(getComputedStyle(node).borderRadius),
    width: node.getBoundingClientRect().width,
    offset: parseFloat(getComputedStyle(node).outlineOffset),
  }));
  expect(geometry.radius).toBeGreaterThan(0);
  expect(geometry.width).toBeLessThanOrEqual(320);
  expect(geometry.offset).toBeLessThan(0);
});

test("external and keyboard tab selections reveal overflow and close focuses a neighbor", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 400 });
  await page.goto("/tests/ui/gap-c-surfaces.html?surface=tabs");
  await page.getByRole("button", { name: "Select last externally" }).click();
  const last = page.getByRole("tab", {
    name: "File src/file-17.ts",
    exact: true,
  });
  await expect(last).toHaveAttribute("aria-selected", "true");
  const visible = await last.evaluate((node) => {
    const tab = node.getBoundingClientRect();
    const strip = node.closest('[role="tablist"]')!.getBoundingClientRect();
    return tab.left >= strip.left && tab.right <= strip.right;
  });
  expect(visible).toBe(true);
  await last.focus();
  await page.keyboard.press("Home");
  const first = page.getByRole("tab", {
    name: "File src/file-0.ts",
    exact: true,
  });
  await expect(first).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await page
    .getByRole("button", { name: "Close file-1.ts", exact: true })
    .click();
  await expect(
    page.getByRole("tab", { name: "File src/file-2.ts", exact: true }),
  ).toBeFocused();
});

test("Find floats without resizing the page and restores browser focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 400 });
  await page.goto("/tests/ui/gap-c-surfaces.html?surface=browser");
  const frame = page.locator(".browser-surface__frame");
  expect(
    (await page.locator(".browser-surface__toolbar").boundingBox())!.height,
  ).toBe(44);
  const before = await frame.boundingBox();
  await page.getByRole("button", { name: "More options" }).click();
  await page
    .getByRole("menuitem", { name: "Find in page", exact: true })
    .click();
  const query = page.getByRole("textbox", { name: "Find text" });
  await expect(query).toBeFocused();
  expect(await frame.boundingBox()).toEqual(before);
  await expect(page.locator("#native-result")).toHaveAttribute(
    "data-covered",
    "false",
  );
  const findBounds = (await page
    .getByRole("form", { name: "Find in page" })
    .boundingBox())!;
  expect(findBounds.y + findBounds.height).toBeLessThanOrEqual(before!.y);
  await query.fill("needle");
  await query.press("Enter");
  await expect(page.locator("#native-result")).toContainText(
    '"query":"needle"',
  );
  await query.press("Escape");
  await expect(query).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "More options" }),
  ).toBeFocused();
});

test("short scaled menu and downloads remain bounded and dismiss locally", async ({
  page,
}) => {
  await page.setViewportSize({ width: 480, height: 260 });
  await page.goto("/tests/ui/gap-c-surfaces.html?surface=browser");
  await page.locator(".workbench").evaluate((node) => {
    (node as HTMLElement).style.zoom = "1.25";
  });
  await page.getByRole("button", { name: "More options" }).click();
  const menu = page.getByRole("menu", { name: "Browser menu" });
  const bounds = (await menu.boundingBox())!;
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(260);
  await menu
    .getByRole("menuitem", { name: "Browser settings", exact: true })
    .focus();
  await expect(
    menu.getByRole("menuitem", { name: "Browser settings", exact: true }),
  ).toBeInViewport();
  await page.keyboard.press("Tab");
  await expect(menu).toHaveCount(0);
  await page
    .getByRole("button", { name: "Downloads, 1 active", exact: true })
    .click();
  const downloads = page.getByRole("dialog", { name: "Downloads" });
  await expect(downloads).toBeVisible();
  const popup = (await downloads.boundingBox())!;
  expect(popup.y + popup.height).toBeLessThanOrEqual(260);
  await downloads.getByRole("button", { name: /Cancel/ }).click();
  await expect(page.locator("#native-result")).toContainText('"id":7');
  await page.keyboard.press("Escape");
  await expect(downloads).toHaveCount(0);
});

test("terminal rethemes rendered output without replacing its input or session", async ({
  page,
}) => {
  await page.goto("/tests/ui/gap-c-surfaces.html?surface=terminal");
  const rows = page.locator(".xterm-rows");
  await expect(rows).toContainText("SAVED OUTPUT");
  const input = page.locator(".xterm-helper-textarea");
  await input.fill("echo retained");
  await page.locator(".workbench").evaluate((node) => {
    const style = (node as HTMLElement).style;
    style.setProperty("--color-canvas", "rgb(30, 40, 50)");
    style.setProperty("--color-text-primary", "rgb(220, 210, 200)");
    style.setProperty("--terminal-emphasis", "rgb(240, 230, 220)");
    style.setProperty("--color-text-emphasis", "var(--terminal-emphasis)");
    style.setProperty(
      "--color-selection",
      "color-mix(in srgb, rgb(80, 100, 120) 50%, rgb(40, 60, 80))",
    );
  });
  await expect(page.locator(".xterm-scrollable-element")).toHaveCSS(
    "background-color",
    "rgb(30, 40, 50)",
  );
  await expect(page.locator(".xterm-cursor-block")).toHaveCSS(
    "background-color",
    "rgb(240, 230, 220)",
  );
  const savedOutput = rows.getByText("SAVED OUTPUT", { exact: true });
  await expect(savedOutput).toBeVisible();
  const text = (await savedOutput.boundingBox())!;
  await page.mouse.move(text.x + 1, text.y + text.height / 2);
  await page.mouse.down();
  await page.mouse.move(text.x + text.width - 1, text.y + text.height / 2);
  await page.mouse.up();
  const selection = page.locator(".xterm-selection > div");
  await expect(selection.first()).toBeVisible();
  for (const part of await selection.all())
    await expect(part).toHaveCSS(
      "background-color",
      "color(srgb 0.235294 0.313726 0.392157)",
    );
  await expect(rows).toContainText("SAVED OUTPUT");
  await input.press("Enter");
  await expect(page.locator("#native-result")).toContainText('"text":"\\r"');
});

test("a failed New tab import retries locally without creating a browser resource", async ({
  page,
}) => {
  await page.route("**/src/features/browser/BrowserNewTab.tsx*", (route) =>
    route.abort(),
  );
  await page.goto("/tests/ui/gap-c-surfaces.html?surface=new-tab");
  await expect(
    page.getByRole("alert", { name: "New tab failed to load" }),
  ).toBeVisible();
  await page.unroute("**/src/features/browser/BrowserNewTab.tsx*");
  await page.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(page.getByText("Tools", { exact: true })).toBeVisible();
  await expect(page.locator("#native-result")).toBeEmpty();
});
