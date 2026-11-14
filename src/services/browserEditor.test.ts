import { describe, expect, it } from "vitest";

import { browserEditor } from "./browserEditor";

describe("browser editor snapshots", () => {
  it("say whether their text is the stored buffer, as native does", () => {
    const resource = { rootId: "root-a", relativePath: "acked-browser.ts" };
    const document = browserEditor.open(resource);
    const snapshot = (content: string, revision: number) =>
      browserEditor.snapshot({
        resource,
        base: document.base,
        lineEnding: document.lineEnding,
        content,
        revision,
      });

    expect(snapshot("two\n", 2)).toMatchObject({ revision: 2, applied: true });
    expect(snapshot("two\n", 2)).toMatchObject({ applied: true });
    expect(snapshot("one\n", 1)).toMatchObject({ revision: 2, applied: false });
    expect(() => snapshot("other\n", 2)).toThrow(
      "another buffer snapshot already holds this revision",
    );
    browserEditor.discard(resource);
  });
});
