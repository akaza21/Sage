import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { brotliDecompressSync } from "node:zlib";
import { expect, it } from "vitest";

const css = readFileSync("src/styles/global.css", "utf8");
const root = css.slice(css.indexOf(":root {"), css.indexOf("@supports"));

/** The hex a `:root` token resolves to through its var() chain. */
function hex(token: string): string {
  const value = root
    .match(new RegExp(`\\n\\s*--${token}:\\s*([^;]+);`))?.[1]
    ?.trim();
  if (!value) throw new Error(`--${token} is not declared`);
  const alias = value.match(/^var\(--([\w-]+)\)$/);
  if (alias) return hex(alias[1]);
  const mix = value
    .replace(/\s+/g, " ")
    .match(
      /^color-mix\( ?in srgb, var\(--([\w-]+)\) ([\d.]+)%, var\(--([\w-]+)\) ?\)$/,
    );
  if (mix) {
    const [a, b, share] = [hex(mix[1]), hex(mix[3]), Number(mix[2]) / 100];
    return `#${[1, 3, 5]
      .map((i) =>
        Math.round(
          parseInt(a.slice(i, i + 2), 16) * share +
            parseInt(b.slice(i, i + 2), 16) * (1 - share),
        )
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")}`;
  }
  const short = value.match(/^#([\da-f])([\da-f])([\da-f])$/i);
  if (short)
    return `#${short
      .slice(1)
      .map((c) => c + c)
      .join("")}`.toLowerCase();
  if (!/^#[\da-f]{6}$/i.test(value)) throw new Error(`--${token}: ${value}`);
  return value.toLowerCase();
}

const luminance = (color: string) =>
  [1, 3, 5]
    .map((start) => parseInt(color.slice(start, start + 2), 16) / 255)
    .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
    .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
const contrast = (a: string, b: string) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

it("resolves the approved neutrals, text, status and accent values", () => {
  expect({
    window: hex("color-canvas"),
    sidebar: hex("agent-shell-sidebar"),
    rail: hex("agent-shell-rail"),
    card: hex("agent-shell-center"),
    raised: hex("color-plane-raised"),
    bubble: hex("agent-shell-user-bubble"),
    composer: hex("composer-fill"),
    composerFrame: hex("composer-frame"),
    selectedRow: hex("agent-shell-selected"),
    primary: hex("color-text-primary"),
    secondary: hex("color-text-secondary"),
    muted: hex("color-text-muted"),
    placeholder: hex("color-text-placeholder"),
    danger: hex("color-danger"),
    warning: hex("color-warning"),
    success: hex("color-success"),
    accent: hex("color-accent"),
  }).toEqual({
    window: "#111114",
    sidebar: "#111114",
    rail: "#111114",
    card: "#070708",
    raised: "#131317",
    bubble: "#131317",
    composer: "#15151a",
    composerFrame: "#0d0d11",
    selectedRow: "#1f1f24",
    primary: "#ffffff",
    secondary: "#bcbdc6",
    muted: "#87888f",
    placeholder: "#80818a",
    danger: "#de5555",
    warning: "#f3c949",
    success: "#7aae66",
    accent: "#4c9df3",
  });
});

it("holds 4.5:1 for every text colour on each surface it is drawn on", () => {
  const panels = ["color-window", "color-card", "color-raised"];
  const composer = ["composer-fill", "composer-frame"];
  const pairings: Record<string, string[]> = {
    "color-text-primary": [...panels, ...composer, "color-row-selected"],
    "color-text-secondary": [...panels, ...composer, "color-row-selected"],
    "color-text-muted": [...panels, ...composer, "color-row-selected"],
    "color-accent": [...panels, ...composer, "color-row-selected"],
    "color-text-placeholder": [...panels, ...composer],
    "color-danger": [...panels, ...composer, "color-danger-muted"],
    "color-warning": [...panels, ...composer, "color-warning-muted"],
    "color-success": [...panels, ...composer, "color-success-muted"],
    "color-code-text": panels,
  };
  const failing = Object.entries(pairings).flatMap(([ink, surfaces]) =>
    surfaces
      .map((surface) => ({
        ink,
        surface,
        ratio: contrast(hex(ink), hex(surface)),
      }))
      .filter(({ ratio }) => ratio < 4.5),
  );
  expect(failing).toEqual([]);
});

it("bundles a variable-weight Google Sans Flex for every @font-face source", () => {
  const sources = [
    ...css.matchAll(/@font-face\s*\{[^}]*?url\("([^"]+)"\)[^}]*\}/g),
  ].map((match) => match[1]);
  expect(sources).toHaveLength(2);
  for (const source of sources) {
    expect(source.startsWith("../assets/fonts/google-sans-flex/")).toBe(true);
    const font = readFileSync(resolve("src/styles", source));
    expect(font.subarray(0, 4).toString("latin1")).toBe("wOF2");
    expect(woff2Axes(font)).toContainEqual(["wght", 1, 400, 1000]);
  }
  expect(
    readFileSync("src/assets/fonts/google-sans-flex/OFL.txt", "utf8"),
  ).toContain("SIL Open Font License, Version 1.1");
  expect(css).toMatch(/--font-family-ui:\s*"Google Sans Flex",/);
  expect(css).toMatch(/--font-family-agent:\s*"Google Sans Flex",/);
});

/** fvar axes as [tag, min, default, max] from a WOFF2 file. */
function woff2Axes(font: Buffer): [string, number, number, number][] {
  const known: Record<number, string> = {
    47: "fvar",
    10: "glyf",
    11: "loca",
    3: "hmtx",
  };
  let at = 48;
  const base128 = () => {
    let value = 0;
    for (let i = 0; i < 5; i++) {
      const byte = font[at++];
      value = value * 128 + (byte & 127);
      if (!(byte & 128)) break;
    }
    return value;
  };
  const offsets = new Map<string, number>();
  let offset = 0;
  for (let i = 0, count = font.readUInt16BE(12); i < count; i++) {
    const flags = font[at++];
    let tag = known[flags & 63] ?? String(flags & 63);
    if ((flags & 63) === 63) {
      tag = font.toString("latin1", at, at + 4);
      at += 4;
    }
    const version = flags >> 6;
    let length = base128();
    if (
      ((tag === "glyf" || tag === "loca") && version === 0) ||
      (tag === "hmtx" && version !== 0)
    )
      length = base128();
    offsets.set(tag, offset);
    offset += length;
  }
  const tables = brotliDecompressSync(font.subarray(at));
  const fvar = tables.subarray(offsets.get("fvar"));
  const axesAt = fvar.readUInt16BE(4);
  const size = fvar.readUInt16BE(10);
  return Array.from({ length: fvar.readUInt16BE(8) }, (_, i) => {
    const axis = fvar.subarray(axesAt + i * size);
    return [
      axis.toString("latin1", 0, 4),
      axis.readInt32BE(4) / 65536,
      axis.readInt32BE(8) / 65536,
      axis.readInt32BE(12) / 65536,
    ];
  });
}
