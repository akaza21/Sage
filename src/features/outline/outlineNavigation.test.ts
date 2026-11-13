import { describe, expect, it } from "vitest";

import { localSymbolNavigation } from "./outlineNavigation";
import { syntaxOutline } from "./syntaxOutline";

const content = "const value = example();\n\nexport function example() {}\n";

describe("localSymbolNavigation", () => {
  it("moves the caret to the declaration named under the caret", async () => {
    const outline = await syntaxOutline("src/example.ts", content);
    expect(
      localSymbolNavigation(content, { start: 16, end: 16 }, outline),
    ).toEqual({
      start: content.indexOf("example() {}"),
      end: content.indexOf("example() {}"),
    });
  });

  it("accepts an exact identifier selection", async () => {
    const outline = await syntaxOutline("src/example.ts", content);
    expect(
      localSymbolNavigation(content, { start: 6, end: 11 }, outline),
    ).toEqual({ start: 6, end: 6 });
  });

  it("fails closed without an Outline or for a name it does not declare", async () => {
    expect(
      localSymbolNavigation(content, { start: 16, end: 16 }, null),
    ).toBeNull();
    const outline = await syntaxOutline(
      "src/example.ts",
      "missing();\nconst other = 1;\n",
    );
    expect(
      localSymbolNavigation(
        "missing();\nconst other = 1;\n",
        { start: 2, end: 2 },
        outline,
      ),
    ).toBeNull();
  });

  it("prefers a top-level declaration over a class member of the same name", async () => {
    const source = "class A {\n  run() {}\n}\nfunction run() {}\nrun();\n";
    const outline = await syntaxOutline("src/example.ts", source);
    const call = source.lastIndexOf("run();");
    const topLevel = source.indexOf("run() {}", source.indexOf("function"));
    expect(
      localSymbolNavigation(source, { start: call, end: call }, outline),
    ).toEqual({ start: topLevel, end: topLevel });
  });

  it("still reaches a member when nothing at the top level declares it", async () => {
    const source = "class A {\n  run() {}\n}\nnew A().run();\n";
    const outline = await syntaxOutline("src/example.ts", source);
    const call = source.lastIndexOf("run();");
    const member = source.indexOf("run() {}");
    expect(
      localSymbolNavigation(source, { start: call, end: call }, outline),
    ).toEqual({ start: member, end: member });
  });
});
