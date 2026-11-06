import { describe, expect, it } from "vitest";
import { decodeCommandList } from "./acpCommandsDesktop";
describe("native command list contract", () => {
  it("preserves advertised hints and empty lists", () => {
    expect(decodeCommandList({ revision: 0, commands: [] })).toEqual({
      revision: 0,
      commands: [],
    });
    const value = {
      revision: 1,
      commands: [{ name: "plan", description: "Plan", inputHint: "task" }],
    };
    expect(decodeCommandList(value)).toEqual(value);
  });
  it("rejects duplicate names, malformed entries, unsafe revisions and oversized lists", () => {
    const command = { name: "plan", description: "Plan", inputHint: null };
    for (const value of [
      null,
      { revision: -1, commands: [] },
      { revision: 2 ** 53, commands: [] },
      { revision: 1, commands: [command, command] },
      { revision: 1, commands: [{ ...command, name: "/plan" }] },
      { revision: 1, commands: Array(129).fill(command) },
      {
        revision: 1,
        commands: [{ ...command, description: "x".repeat(1025) }],
      },
    ])
      expect(decodeCommandList(value)).toBeNull();
  });
});
