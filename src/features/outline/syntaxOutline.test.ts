import { describe, expect, it, vi } from "vitest";

import { MAX_OUTLINE_SOURCE_CHARS } from "./outlineAvailability";
import {
  MAX_OUTLINE_DEPTH,
  MAX_OUTLINE_SYMBOLS,
  syntaxOutline,
  type OutlineSymbol,
} from "./syntaxOutline";

function entries(symbols: readonly OutlineSymbol[] | undefined) {
  return (symbols ?? []).map(
    ({ name, kind, line, depth }) =>
      `${"  ".repeat(depth)}${line} ${kind} ${name}`,
  );
}

describe("syntaxOutline", () => {
  it("lists TypeScript declarations, nesting class and interface members", async () => {
    const source = [
      'import value from "./value";',
      "export function first(a: number) {",
      "  function nested() {}",
      "  return a;",
      "}",
      "export default function () {}",
      "class Second { method() {} }",
      "export interface Third { a: string }",
      "type Fourth = string;",
      "enum Fifth { A }",
      "const sixth = 1, seventh = () => 2;",
      "export const eighth = 3;",
      "namespace Ninth {}",
      "async function* tenth() {}",
    ].join("\n");
    const outline = await syntaxOutline("src/example.ts", source);
    expect(entries(outline?.symbols)).toEqual([
      "2 function first",
      "6 function default",
      "7 class Second",
      "  7 function method",
      "8 type Third",
      "  8 property a",
      "9 type Fourth",
      "10 type Fifth",
      "11 variable sixth",
      "11 variable seventh",
      "12 variable eighth",
      "13 module Ninth",
      "14 function tenth",
    ]);
    expect(outline?.truncated).toBe(false);
    expect(outline?.symbols[0]?.offset).toBe(source.indexOf("first"));
  });

  it("reads JSX and JavaScript files with the same grammar family", async () => {
    const outline = await syntaxOutline(
      "src/App.jsx",
      "export function App() { return <div />; }\nconst x = 1;\n",
    );
    expect(entries(outline?.symbols)).toEqual([
      "1 function App",
      "2 variable x",
    ]);
  });

  it("lists Python functions and classes, through decorators", async () => {
    const outline = await syntaxOutline(
      "tool.py",
      "import os\n\n@cache\ndef first():\n    pass\n\nclass Second:\n    def method(self):\n        pass\n\nasync def third():\n    pass\n",
    );
    expect(entries(outline?.symbols)).toEqual([
      "4 function first",
      "7 class Second",
      "  8 function method",
      "11 function third",
    ]);
  });

  it("lists Rust items, naming impl blocks by their header", async () => {
    const outline = await syntaxOutline(
      "src/lib.rs",
      [
        "use std::fmt;",
        "pub fn first() {}",
        "struct Point { x: u8 }",
        "impl fmt::Display for Point {",
        "    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { Ok(()) }",
        "}",
        "const LIMIT: u8 = 1;",
        "mod inner {}",
        "macro_rules! twice { () => {} }",
      ].join("\n"),
    );
    expect(entries(outline?.symbols)).toEqual([
      "2 function first",
      "3 type Point",
      "4 type impl fmt::Display for Point",
      "  5 function fmt",
      "7 variable LIMIT",
      "8 module inner",
      "9 function twice",
    ]);
  });

  it("stops nesting at the depth cap", async () => {
    const levels = MAX_OUTLINE_DEPTH + 2;
    const source =
      Array.from({ length: levels }, (_, index) => `mod m${index} {`).join(
        "\n",
      ) + "}".repeat(levels);
    const outline = await syntaxOutline("src/deep.rs", source);
    expect(outline?.symbols.map(({ name, depth }) => [name, depth])).toEqual(
      Array.from({ length: MAX_OUTLINE_DEPTH }, (_, index) => [
        `m${index}`,
        index,
      ]),
    );
  });

  it("lists Markdown headings without their marks, nested by level", async () => {
    const outline = await syntaxOutline(
      "README.md",
      "# Title\n\nText\n\n## Second *part* ##\n\nSetext\n===\n",
    );
    expect(entries(outline?.symbols)).toEqual([
      "1 heading Title",
      "  5 heading Second *part*",
      "7 heading Setext",
    ]);
  });

  it("lists top-level JSON keys and CSS rules", async () => {
    const json = await syntaxOutline(
      "package.json",
      '{\n  "name": "x",\n  "scripts": { "test": "y" }\n}\n',
    );
    expect(entries(json?.symbols)).toEqual([
      '2 property "name"',
      '3 property "scripts"',
    ]);
    const css = await syntaxOutline(
      "app.css",
      "a,\n.b > c { color: red }\n@media (width > 1px) { a {} }\n",
    );
    expect(entries(css?.symbols)).toEqual([
      "1 rule a, .b > c",
      "3 rule @media (width > 1px)",
    ]);
  });

  it("counts columns in code points, as File navigation does", async () => {
    const outline = await syntaxOutline(
      "src/emoji.ts",
      "/* \u{1F600} */ function foo() {}\nconst bar = 1;\n",
    );
    expect(
      outline?.symbols.map(({ name, line, column, offset }) => [
        name,
        line,
        column,
        offset,
      ]),
    ).toEqual([
      ["foo", 1, 18, 18],
      ["bar", 2, 7, 33],
    ]);
  });

  it("stops reading a large JSON object at the cap", async () => {
    const keys = Array.from(
      { length: MAX_OUTLINE_SYMBOLS + 10 },
      (_, index) => `"k${index}": ${index}`,
    );
    const outline = await syntaxOutline("big.json", `{${keys.join(",")}}`);
    expect(outline?.symbols).toHaveLength(MAX_OUTLINE_SYMBOLS);
    expect(outline?.symbols.at(-1)?.name).toBe(`"k${MAX_OUTLINE_SYMBOLS - 1}"`);
    expect(outline?.truncated).toBe(true);
  });

  it("returns no Outline for languages without one or documents over the editable limit", async () => {
    await expect(syntaxOutline("notes.txt", "text")).resolves.toBeNull();
    await expect(syntaxOutline("index.html", "<h1>x</h1>")).resolves.toBeNull();
    await expect(
      syntaxOutline("big.ts", "x".repeat(MAX_OUTLINE_SOURCE_CHARS + 1)),
    ).resolves.toBeNull();
  });

  it("caps the symbol list and says so", async () => {
    const source = Array.from(
      { length: MAX_OUTLINE_SYMBOLS + 1 },
      (_, index) => `const value${index} = ${index};`,
    ).join("\n");
    const outline = await syntaxOutline("many.ts", source);
    expect(outline?.symbols).toHaveLength(MAX_OUTLINE_SYMBOLS);
    expect(outline?.truncated).toBe(true);
  });

  it("drops a result whose request is no longer current", async () => {
    await expect(
      syntaxOutline("a.ts", "const a = 1;", () => false),
    ).resolves.toBeNull();
  });

  it("parses a large File in slices and reparses only what an edit changed", async () => {
    const unit =
      "export function f%(a: number) {\n  return a * 2;\n}\nclass C% {}\ncall(%);\n";
    let source = "";
    for (let index = 0; source.length < 400_000; index += 1) {
      source += unit.replaceAll("%", String(index));
    }
    const yields = () =>
      timeout.mock.calls.filter(([, delay]) => delay === 0).length;
    const timeout = vi.spyOn(globalThis, "setTimeout");
    try {
      await syntaxOutline("large.ts", source);
      const cold = yields();
      timeout.mockClear();
      const middle = source.length / 2;
      const edited = `${source.slice(0, middle)}x${source.slice(middle)}`;
      const outline = await syntaxOutline("large.ts", edited);
      const incremental = yields();

      // A cold parse of this size cannot finish inside one slice, so the main
      // thread must have been released between slices.
      expect(cold).toBeGreaterThan(4);
      // Reusing the unchanged tree makes the edit at least 4x cheaper.
      expect(incremental * 4).toBeLessThan(cold);
      expect(outline?.symbols).toHaveLength(MAX_OUTLINE_SYMBOLS);
    } finally {
      timeout.mockRestore();
    }
  });

  it("yields and can stop inside one large namespace, not only between top-level nodes", async () => {
    const source = `namespace N {\n${"call();\n".repeat(5_000)}}\n`;
    // Parse once so the runs below measure only the symbol walk.
    await syntaxOutline("namespace.ts", source);
    // Hundreds of forced yields must not depend on the host timer resolution.
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    // A clock that advances 1ms per read makes every slice a few nodes long.
    let now = 0;
    const clock = vi
      .spyOn(performance, "now")
      .mockImplementation(() => (now += 1));
    const timeout = vi.spyOn(globalThis, "setTimeout");
    const yields = () =>
      timeout.mock.calls.filter(([, delay]) => delay === 0).length;
    try {
      const pending = syntaxOutline("namespace.ts", source);
      await vi.runAllTimersAsync();
      const outline = await pending;
      expect(entries(outline?.symbols)).toEqual(["1 module N"]);
      expect(yields()).toBeGreaterThan(100);

      timeout.mockClear();
      let checks = 0;
      // Current through the language load, stale at the walk's first yield.
      const stopping = syntaxOutline(
        "namespace.ts",
        source,
        () => (checks += 1) === 1,
      );
      await vi.runAllTimersAsync();
      const stopped = await stopping;
      expect(stopped).toBeNull();
      expect(yields()).toBe(1);
    } finally {
      clock.mockRestore();
      timeout.mockRestore();
      vi.useRealTimers();
    }
  });
});
