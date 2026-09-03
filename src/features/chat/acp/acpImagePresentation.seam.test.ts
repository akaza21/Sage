import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("ACP image presentation stylesheet seam", () => {
  it("contains admitted images without stretching, cropping, or motion", async () => {
    const css = await readFile(
      resolve("src/features/chat/acp/acp-chat-activities.css"),
      "utf8",
    );
    const figure = css.match(/\.acp-image\s*\{(?<rules>[^}]*)\}/)?.groups
      ?.rules;
    const image = css.match(/\.acp-image__src\s*\{(?<rules>[^}]*)\}/)?.groups
      ?.rules;

    expect(figure).toMatch(/max-width:\s*100%;/);
    expect(figure).toMatch(/overflow:\s*hidden;/);
    expect(image).toMatch(/max-width:\s*100%;/);
    expect(image).toMatch(/max-height:\s*min\(60vh,\s*520px\);/);
    expect(image).toMatch(/width:\s*auto;/);
    expect(image).toMatch(/height:\s*auto;/);
    expect(image).toMatch(/object-fit:\s*contain;/);
    expect(`${figure}\n${image}`).not.toMatch(/animation|transition|scale/);
  });

  it("retains a visible figure boundary in forced colors", async () => {
    const css = await readFile(
      resolve("src/features/chat/acp/acp-chat-activities.css"),
      "utf8",
    );

    expect(css).toMatch(
      /@media \(forced-colors: active\)[\s\S]*?\.acp-image[\s\S]*?border-color:\s*CanvasText;/,
    );
  });
});
