import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

describe("Launch canvas color", () => {
  it("paints the window and the HTML shell in the app canvas color before the bundle loads", async () => {
    const [css, html, config] = await Promise.all([
      readFile("src/styles/global.css", "utf8"),
      readFile("index.html", "utf8"),
      readFile("src-tauri/tauri.conf.json", "utf8"),
    ]);
    const canvas = css.match(/--color-canvas:\s*(#[0-9a-f]{6});/i)?.[1];
    expect(canvas).toBeDefined();

    const shellStyle = html.match(
      /<head>[\s\S]*<style>([\s\S]*?)<\/style>[\s\S]*<\/head>/,
    )?.[1];
    expect(shellStyle).toMatch(
      new RegExp(`html,\\s*body\\s*\\{\\s*background:\\s*${canvas};`, "i"),
    );

    const mainWindow = (
      JSON.parse(config) as {
        app: { windows: { label: string; backgroundColor?: string }[] };
      }
    ).app.windows.find((window) => window.label === "main");
    expect(mainWindow?.backgroundColor?.toLowerCase()).toBe(
      canvas!.toLowerCase(),
    );
  });
});
