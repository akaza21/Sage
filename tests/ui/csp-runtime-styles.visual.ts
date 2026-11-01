import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";

import { expect, test, type Page } from "@playwright/test";

import { openScenario } from "./shared/visualBrowserPrimitives";

// The packaged app runs under tauri.conf.json's CSP as Tauri serves it: every
// `<style>` in index.html gets a fresh nonce that is added to style-src, which
// makes the browser ignore 'unsafe-inline', and inline scripts are allowed by
// hash. Styles a library injects at runtime then need that nonce, or they are
// refused and CodeMirror runs without its base theme.
const tauriConfig = JSON.parse(
  readFileSync(
    new URL("../../src-tauri/tauri.conf.json", import.meta.url),
    "utf8",
  ),
) as { app: { security: { csp: Record<string, string> } } };

const NBSP = " ";

async function serveUnderTauriCsp(page: Page) {
  const nonce = BigInt(`0x${randomBytes(8).toString("hex")}`).toString();
  await page.route(
    (url) => url.pathname === "/",
    async (route) => {
      const response = await route.fetch();
      const scriptHashes: string[] = [];
      const html = (await response.text())
        .replace(/<style(?![^>]*\snonce=)/gu, `<style nonce="${nonce}"`)
        .replace(
          /<script(?![^>]*\ssrc=)[^>]*>([\s\S]*?)<\/script>/gu,
          (script, body: string) => {
            const digest = createHash("sha256").update(body).digest("base64");
            scriptHashes.push(`'sha256-${digest}'`);
            return script;
          },
        )
        // Vite's dev server injects each CSS module as a <style>; this is
        // its documented way to give them the nonce. Packaged builds link CSS.
        .replace(
          "<head>",
          `<head><meta property="csp-nonce" nonce="${nonce}" />`,
        );
      const directives = { ...tauriConfig.app.security.csp };
      directives["style-src"] += ` 'nonce-${nonce}'`;
      directives["script-src"] += ` ${scriptHashes.join(" ")}`;
      // Vite's HMR socket only; unrelated to styles.
      directives["connect-src"] +=
        ` ws://${new URL(route.request().url()).host}`;
      await route.fulfill({
        response,
        body: html,
        headers: {
          ...response.headers(),
          "content-security-policy": Object.entries(directives)
            .map(([name, sources]) => `${name} ${sources}`)
            .join("; "),
        },
      });
    },
  );
  await page.addInitScript(() => {
    const refused: string[] = [];
    (window as unknown as { refusedStyles: string[] }).refusedStyles = refused;
    document.addEventListener("securitypolicyviolation", (event) => {
      if (event.effectiveDirective.startsWith("style-src"))
        refused.push(`${event.effectiveDirective} ${event.sourceFile}`);
    });
  });
}

test("the editor keeps its layout and plain spaces under the packaged CSP", async ({
  page,
}) => {
  await serveUnderTauriCsp(page);
  await page.setViewportSize({ width: 1380, height: 880 });
  await openScenario(page, "editor", "editor");
  const editor = page.locator(".cm-editor");
  const firstLine = editor.locator(".cm-line").first();
  const firstNumber = editor
    .locator(".cm-lineNumbers .cm-gutterElement")
    .filter({ hasText: /^1$/u });
  await expect(firstLine).toBeVisible();
  await expect(firstNumber).toBeVisible();

  const lineTop = (await firstLine.boundingBox())!.y;
  const numberTop = (await firstNumber.boundingBox())!.y;
  expect(Math.abs(lineTop - numberTop)).toBeLessThanOrEqual(1);

  await editor.locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.press("Enter");
  await page.keyboard.type("let typed = 1;");
  const typed = editor.locator(".cm-line").filter({ hasText: "typed" });
  await expect(typed).toHaveCount(1);
  const text = await typed.evaluate((line) => line.textContent ?? "");
  expect(text.includes(NBSP)).toBe(false);
  expect(text.trim()).toBe("let typed = 1;");

  expect(
    await page.evaluate(
      () => (window as unknown as { refusedStyles: string[] }).refusedStyles,
    ),
  ).toEqual([]);
});
