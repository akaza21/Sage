import { describe, expect, it } from "vitest";
import { decodeCommandList, decodeCommandsNotice } from "./acpCommandsDesktop";
describe("native command list contract", () => {
  it("preserves advertised hints and empty lists", () => {
    expect(decodeCommandList({ revision: 0, commands: [] })).toEqual({
      revision: 0,
      commands: [],
    });
    const value = {
      revision: 1,
      commands: [
        {
          name: "plan",
          description: "Plan",
          inputHint: "task",
          source: "skill",
        },
      ],
    };
    expect(decodeCommandList(value)).toEqual(value);
    // A command that opens a panel names it.
    const status = { ...value.commands[0]!, name: "status", panel: "status" };
    expect(
      decodeCommandList({ revision: 2, commands: [status] })?.commands[0]
        ?.panel,
    ).toBe("status");
  });
  it("rejects duplicate names, malformed entries, unsafe revisions and oversized lists", () => {
    const command = {
      name: "plan",
      description: "Plan",
      inputHint: null,
      source: "harness",
    };
    for (const value of [
      null,
      { revision: -1, commands: [] },
      { revision: 2 ** 53, commands: [] },
      { revision: 1, commands: [command, command] },
      { revision: 1, commands: [{ ...command, name: "/plan" }] },
      { revision: 1, commands: [{ ...command, source: "invented" }] },
      { revision: 1, commands: [{ ...command, source: undefined }] },
      { revision: 1, commands: [{ ...command, panel: "settings" }] },
      { revision: 1, commands: [{ ...command, panel: null }] },
      { revision: 1, commands: Array(513).fill(command) },
      {
        revision: 1,
        commands: [{ ...command, description: "x".repeat(1025) }],
      },
    ])
      expect(decodeCommandList(value)).toBeNull();
  });
  it("admits only a well-formed change notice", () => {
    const notice = {
      schemaVersion: 1,
      sageProjectId: "project",
      sageChatId: "chat",
      sageAgentSessionId: "session",
      runtimeEpoch: "1",
      revision: 1,
    };
    const { schemaVersion: _version, ...decoded } = notice;
    expect(decodeCommandsNotice(notice)).toEqual(decoded);
    for (const value of [
      { ...notice, schemaVersion: 2 },
      { ...notice, sageChatId: "" },
      { ...notice, runtimeEpoch: 1 },
      { ...notice, runtimeEpoch: "0" },
      { ...notice, revision: 0 },
    ])
      expect(decodeCommandsNotice(value)).toBeNull();
  });
});
