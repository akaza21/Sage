import { describe, expect, it } from "vitest";

import type { AcpChatPage } from "../domain/acp";
import { utf8ByteLength } from "../domain/text";
import { pageByteLength } from "./acpProjectionBytes";

const item = (id: string, text: string) => ({ id, kind: "text", text });

function page(turns: unknown[]): AcpChatPage {
  return {
    projectId: "p",
    chatId: "c",
    title: "Tïtle ✓",
    nextCursor: null,
    turns,
  } as unknown as AcpChatPage;
}

const exact = (value: AcpChatPage) => utf8ByteLength(JSON.stringify(value));

describe("pageByteLength", () => {
  it("matches the serialized size for empty, single and multi-entry pages", () => {
    const shared = item("a", "héllo 🌍");
    const cases = [
      page([]),
      page([{ id: "t0", items: [], activities: [] }]),
      page([
        {
          id: "t3",
          items: [item("d", "middle")],
          activities: [{ id: "y", note: undefined }],
          browserOperations: [{ id: "op" }],
          terminalAt: undefined,
        },
      ]),
      page([
        { id: "t1", items: [shared], activities: [{ id: "x", n: 1 }] },
        {
          id: "t2",
          items: [shared, item("b", '"quoted"\n'), item("c", "")],
          activities: [],
        },
      ]),
    ];
    for (const value of cases) expect(pageByteLength(value)).toBe(exact(value));
  });

  it("stays exact when a cached entry is reused in a trimmed page", () => {
    const items = Array.from({ length: 5 }, (_, i) =>
      item(`i${i}`, "x".repeat(i)),
    );
    const full = page([{ id: "t", items, activities: [] }]);
    expect(pageByteLength(full)).toBe(exact(full));
    const trimmed = page([{ id: "t", items: items.slice(2), activities: [] }]);
    expect(pageByteLength(trimmed)).toBe(exact(trimmed));
  });
});
