import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";

import type { Page } from "@playwright/test";

// The packaged app runs under tauri.conf.json's CSP as Tauri serves it: every
// `<style>` in index.html gets a fresh nonce that is added to style-src, which
// makes the browser ignore 'unsafe-inline', and inline scripts are allowed by
// hash. Styles a library injects at runtime then need that nonce, or they are
// refused and CodeMirror/xterm run without their layout styles.
const tauriConfig = JSON.parse(
  readFileSync(
    new URL("../../../src-tauri/tauri.conf.json", import.meta.url),
    "utf8",
  ),
) as { app: { security: { csp: Record<string, string> } } };

export async function serveUnderTauriCsp(page: Page) {
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
}
