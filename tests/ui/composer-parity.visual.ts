import { waitForAnimations } from "./shared/visualBrowserPrimitives";
import { expect, test, type Page } from "@playwright/test";
import { build } from "vite";
import type { ParityState } from "./fixtures/composerParityHarness";
import { expectCompactComposerTargets } from "./shared/composerGeometry";

let script: string;
let styles: string;
test.beforeAll(async () => {
  const result = await build({
    configFile: false,
    publicDir: false,
    logLevel: "silent",
    resolve: { preserveSymlinks: true },
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
    build: {
      write: false,
      lib: {
        entry: "tests/ui/fixtures/composerParityHarness.tsx",
        formats: ["iife"],
        name: "ComposerParity",
      },
    },
  });
  const output = (Array.isArray(result) ? result : [result]).flatMap(
    (bundle) => {
      if (!("output" in bundle)) throw new Error("Expected fixture output");
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
async function render(page: Page, name: string, state: ParityState) {
  await page.evaluate(
    ({ name, state }) =>
      (
        window as typeof window & {
          renderComposerParity(name: string, state: ParityState): void;
        }
      ).renderComposerParity(name, state),
    { name, state },
  );
  await expect(page.locator("form.composer")).toBeVisible();
}
async function noClip(page: Page, selector: string) {
  for (const element of await page.locator(selector).all()) {
    const geometry = await element.evaluate((element) => ({
      width: element.clientWidth,
      scroll: element.scrollWidth,
    }));
    expect(geometry.scroll).toBeLessThanOrEqual(geometry.width + 1);
  }
}

for (const width of [1888, 720, 480]) {
  test.describe(`${width}px composer parity without a server`, () => {
    test.beforeEach(async ({ page }) => {
      await page.setViewportSize({ width, height: 1096 });
      await page.setContent(
        `<style>${styles}</style><main id="root" style="display:grid;width:100%;height:100%"></main>`,
      );
      await page.addScriptTag({ content: script });
    });
    for (const name of ["Codex", "Claude Code", "Pi", "OpenCode", "Oh My Pi"]) {
      test(`${name}: confirmed labels, distinct targets and a reset survive interrupt`, async ({
        page,
      }) => {
        await render(page, name, "running");
        const composer = page.locator("form.composer");
        await expectCompactComposerTargets(composer);
        await render(page, name, "stopped");
        const model = composer.getByRole("button", {
          name: "Model",
          exact: true,
        });
        const effort = composer.getByRole("button", {
          name: "Thinking level: High",
        });
        await expect(model).toHaveText("Confirmed session model");
        await expect(model.locator(".lucide-sparkles")).toHaveCount(1);
        await expect(effort.locator(".lucide-brain")).toHaveCount(1);
        await expectCompactComposerTargets(composer);
        await effort.click();
        const reset = page.getByRole("button", { name: "Reset effort" });
        await expect(reset).toBeVisible();
        await reset.click();
        await expect(page.getByRole("slider")).toHaveAttribute(
          "aria-valuetext",
          "Low",
        );
      });
    }
    test("new-chat caption reflows and project scope attaches above the composer input", async ({
      page,
    }) => {
      await render(page, "Pi", "home");
      const home = page.getByRole("region", { name: "New chat", exact: true });
      await expect(
        home.getByRole("button", {
          name: "Project: Sage parity project with a long but authoritative workspace name",
        }),
      ).toBeVisible();
      await noClip(page, ".chat-home .searchable-picker__trigger-label");
      const row = await page
        .getByRole("group", { name: "Project scope" })
        .boundingBox();
      const composer = page.locator("form.composer");
      const box = await composer.locator(".composer__body").boundingBox();
      expect(row!.y + row!.height).toBe(box!.y);
      await expect(
        composer.getByRole("group", { name: "Project scope" }),
      ).toHaveCount(1);
      await expect(composer.locator(".project-scope")).toHaveCount(0);
      await expectCompactComposerTargets(composer);
    });
    test("Add descriptions are secondary lines and advertised modes are exclusive", async ({
      page,
    }) => {
      await render(page, "Claude Code", "home");
      const trigger = page.getByRole("button", {
        name: "Composer options",
        exact: true,
      });
      await trigger.click();
      const menu = page.getByRole("menu", { name: "Composer options menu" });
      await expect(menu.getByText("Add", { exact: true })).toHaveCSS(
        "text-transform",
        "none",
      );
      await expect(menu.getByRole("menuitemcheckbox")).toHaveCount(0);
      const advertisedDefault = menu.getByRole("menuitemradio", {
        name: "Default",
      });
      await expect(advertisedDefault).toHaveAttribute("aria-checked", "true");
      const plan = menu.getByRole("menuitemradio", { name: "Plan mode" });
      await waitForAnimations(menu);
      const label = await plan.locator("span").first().boundingBox();
      const description = await plan
        .getByText("Plan mode is off")
        .boundingBox();
      expect(description!.y).toBeGreaterThanOrEqual(label!.y + label!.height);
      await plan.click();
      await trigger.click();
      await expect(plan).toHaveAttribute("aria-checked", "true");
      await expect(advertisedDefault).toHaveAttribute("aria-checked", "false");
      // Choosing the current mode again is not a toggle; it stays selected.
      await plan.click();
      await trigger.click();
      await expect(plan).toHaveAttribute("aria-checked", "true");
      await advertisedDefault.click();
      await trigger.click();
      await expect(plan).toHaveAttribute("aria-checked", "false");
      await expect(advertisedDefault).toHaveAttribute("aria-checked", "true");
    });
    test("only the confirmed harness row has a separate model and effort subtitle", async ({
      page,
    }) => {
      await render(page, "Pi", "home");
      await page.getByRole("button", { name: "Harness", exact: true }).click();
      const selected = page.getByRole("option", {
        name: "Pi — available",
        exact: true,
      });
      const subtitle = selected.locator(".searchable-picker__subtitle");
      await expect(subtitle).toBeVisible();
      await waitForAnimations(page.locator(".composer__harness-menu"));
      const label = await selected
        .locator(".searchable-picker__option-copy > span")
        .first()
        .boundingBox();
      const sub = await subtitle.boundingBox();
      expect(sub!.y).toBeGreaterThanOrEqual(label!.y + label!.height);
      await expect(
        page.locator(".composer__harness-menu .searchable-picker__subtitle"),
      ).toHaveCount(1);
    });
    for (const forcedColors of ["none", "active"] as const) {
      test(`harness focus is keyboard-only and access glyphs agree (${forcedColors})`, async ({
        page,
      }) => {
        await page.emulateMedia({ forcedColors });
        await render(page, "Pi", "home");
        const harness = page.getByRole("button", {
          name: "Harness",
          exact: true,
        });
        await harness.evaluate((element) => element.focus());
        await expect(harness).toHaveCSS("outline-style", "none");
        await harness.click();
        await page
          .getByRole("option", { name: "Pi — available", exact: true })
          .click();
        await expect(harness).toBeFocused();
        await expect(harness).toHaveCSS("outline-style", "none");
        await page.getByRole("textbox", { name: "Message agent" }).click();
        await page.keyboard.press("Tab");
        await page.keyboard.press("Tab");
        await page.keyboard.press("Tab");
        await expect(harness).toBeFocused();
        expect(
          await harness.evaluate((element) =>
            element.matches(":focus-visible"),
          ),
        ).toBe(true);
        expect(
          await harness.evaluate((element) =>
            Number.parseFloat(getComputedStyle(element).outlineWidth),
          ),
        ).toBeGreaterThan(0);
        const access = page.getByRole("button", {
          name: "Access options",
          exact: true,
        });
        await expect(access.locator(".lucide-eye")).toHaveCount(1);
        await access.click();
        await expect(
          page
            .getByRole("menuitemradio", { name: "Read-only", exact: true })
            .locator(".lucide-eye"),
        ).toHaveCount(1);
      });
    }
    test("files-pill counts and table cells never gain an inner horizontal scrollbar", async ({
      page,
    }) => {
      await render(page, "Codex", "files");
      const pill = page.locator(".acp-file-changes__header");
      await expect(pill).toBeVisible();
      await waitForAnimations(page.locator(".acp-file-changes"));
      // Read both children in one frame after the shared pill settles.
      const alignment = await pill.evaluate((element) => {
        const label = element.querySelector("strong")!.getBoundingClientRect();
        const counts = element
          .querySelector('[aria-label="Line changes"]')!
          .getBoundingClientRect();
        return Math.abs(
          label.y + label.height / 2 - counts.y - counts.height / 2,
        );
      });
      expect(alignment).toBeLessThanOrEqual(1);
      await noClip(page, ".acp-file-changes__totals");
      await render(page, "Claude Code", "answer");
      await expect(
        page.getByRole("cell", { name: "Verified replay" }),
      ).toBeVisible();
      await noClip(
        page,
        ".markdown-table, .markdown-table th, .markdown-table td",
      );
    });
    test("pre-turn chips remain truthful and compact without truncation", async ({
      page,
    }) => {
      await render(page, "Claude Code", "pre-turn");
      const composer = page.locator("form.composer");
      await expect(
        composer.getByRole("button", { name: "Model", exact: true }),
      ).toHaveText("Model");
      const effort = composer.getByRole("button", {
        name: "Thinking level",
        exact: true,
      });
      await expect(effort).toBeDisabled();
      await expect(effort).toHaveAttribute(
        "title",
        "Available when the harness advertises effort controls for this session.",
      );
      await expectCompactComposerTargets(composer);
      await expect(
        composer.getByRole("button", { name: "Send message" }),
      ).toBeDisabled();
    });
    test("model groups use human labels and retain exact selection IDs", async ({
      page,
    }) => {
      await render(page, "Pi", "models");
      await page.getByRole("button", { name: "Model", exact: true }).click();
      const recommended = page.getByRole("group", {
        name: "Recommended by harness",
      });
      await expect(
        recommended.getByRole("option", {
          name: "Claude Sonnet 4.5",
          exact: true,
        }),
      ).toHaveAttribute("aria-selected", "true");
      const other = page
        .getByRole("group", { name: "Other models" })
        .getByRole("option", { name: "GPT-5.4 Mini", exact: true });
      await expect(other).toHaveAttribute("data-option-id", "gpt-5.4-mini");
      await other.click();
      await expect(
        page.getByRole("button", { name: "Model", exact: true }),
      ).toHaveText("GPT-5.4 Mini");
    });
    test("supported workspace-write escalation requires confirmation, with safe cancel focus", async ({
      page,
    }) => {
      await render(page, "Codex", "access");
      const access = page.getByRole("button", {
        name: "Access options",
        exact: true,
      });
      await access.click();
      await expect(
        page.getByRole("menuitemradio", { name: "Full access", exact: true }),
      ).toHaveCount(0);
      await page
        .getByRole("menuitemradio", { name: "Workspace edits", exact: true })
        .click();
      const dialog = page.getByRole("dialog", {
        name: "Switch to Workspace edits?",
      });
      await expect(dialog).toBeVisible();
      await expect(
        dialog.getByRole("button", { name: "Cancel", exact: true }),
      ).toBeFocused();
      await expect(access).toHaveAttribute("data-selected-id", "readOnly");
      await page.keyboard.press("Escape");
      await expect(dialog).toHaveCount(0);
      await expect(access).toBeFocused();
      await access.click();
      await page
        .getByRole("menuitemradio", { name: "Workspace edits", exact: true })
        .click();
      await dialog
        .getByRole("button", { name: "Switch access", exact: true })
        .click();
      await expect(access).toHaveAttribute(
        "data-selected-id",
        "workspaceWrite",
      );
      await expect(access).toBeFocused();
    });
    for (const forcedColors of ["none", "active"] as const) {
      test(`fixture model snapshot identities remain visible (${forcedColors})`, async ({
        page,
      }) => {
        await page.emulateMedia({ forcedColors });
        expect(await page.evaluate(() => window.devicePixelRatio)).toBe(1);
        await render(page, "Claude Code", "model-collisions");
        const trigger = page.getByRole("button", {
          name: "Model",
          exact: true,
        });
        await trigger.focus();
        await page.keyboard.press("Enter");
        const menu = page.locator(".composer__model-menu");
        const first = menu.getByRole("option", {
          name: "Claude Sonnet 4 (claude-sonnet-4-20250514)",
          exact: true,
        });
        const second = menu.getByRole("option", {
          name: "Claude Sonnet 4 (claude-sonnet-4-20250601)",
          exact: true,
        });
        await expect(first).toHaveAttribute("aria-selected", "true");
        await expect(
          first.getByText("claude-sonnet-4-20250514", { exact: true }),
        ).toBeVisible();
        await expect(
          second.getByText("claude-sonnet-4-20250601", { exact: true }),
        ).toBeVisible();
        const subtitles = menu.locator(".searchable-picker__subtitle");
        await expect(subtitles).toHaveCount(3);
        await menu.getByRole("option").last().scrollIntoViewIfNeeded();
        await noClip(
          page,
          ".composer__model-menu, .composer-model-picker__list, .composer-model-picker .searchable-picker__subtitle",
        );
        await first.scrollIntoViewIfNeeded();
        await page.keyboard.press("Home");
        await expect(first).toBeFocused();
        await expect
          .poll(() =>
            first.evaluate((element) => element.matches(":focus-visible")),
          )
          .toBe(true);
        await expect(menu).toHaveScreenshot(
          `model-collisions-${width}-${forcedColors}.png`,
        );
        await page.keyboard.press("ArrowDown");
        await expect(second).toBeFocused();
        await page.keyboard.press("Enter");
        await expect(menu).toHaveCount(0);
        await expect(trigger).toBeFocused();
        await page.keyboard.press("Enter");
        await expect(
          menu.getByRole("option", {
            name: "Claude Sonnet 4 (claude-sonnet-4-20250601)",
            exact: true,
          }),
        ).toHaveAttribute("aria-selected", "true");
      });
    }
    test("unconfirmed defaults and unsupported effort never invent controls", async ({
      page,
    }) => {
      await render(page, "Pi", "no-default");
      await page.getByRole("button", { name: "Thinking level: High" }).click();
      await expect(
        page.getByRole("button", { name: "Reset effort" }),
      ).toHaveCount(0);
      await render(page, "OpenCode", "no-effort");
      await expect(page.locator(".composer__effort-picker")).toHaveCount(0);
      await render(page, "Pi", "unknown");
      await expect(
        page.getByRole("button", { name: "Model", exact: true }),
      ).toHaveText("Model");
      await page.getByRole("button", { name: "Harness", exact: true }).click();
      await expect(page.locator(".searchable-picker__subtitle")).toHaveCount(0);
    });
  });
}
