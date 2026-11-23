import { expect, it } from "vitest";
import type { AcpCommandList } from "./acpCommandsDesktop";
import { terminalOnlyCommand } from "./terminalHandoff";

// The handoff as Rust projects it for a curated binding.
const handoff = {
  label: "Example",
  command: 'agent --resume "s-1"',
  terminalOnly: ["hooks", "permissions", "allowed-tools"],
};
const command = 'agent --resume "s-1"';
const list = (
  commands: AcpCommandList["commands"],
  terminalHandoff: unknown = handoff,
): AcpCommandList => ({ revision: 1, commands, terminalHandoff });
const row = (name: string, extra = {}) => ({
  name,
  description: "",
  inputHint: null,
  source: "harness" as const,
  ...extra,
});

it.each([
  ["a reported command", list([row("hooks")]), "hooks", null],
  ["a reported alias", list([row("x", { aliases: ["hooks"] })]), "hooks", null],
  ["a skill", list([row("hooks", { source: "skill" })]), "hooks", null],
  ["an unknown name", list([]), "unknown", null],
  ["a terminal-only command", list([]), "hooks", command],
  ["a curated alias", list([]), "allowed-tools", command],
  ["no handoff", list([], null), "hooks", null],
  ["a malformed handoff", list([], { ...handoff, command: 7 }), "hooks", null],
])(
  "catches %s by the reported list and curation",
  (_, input, name, expected) => {
    expect(terminalOnlyCommand(input, name)).toEqual(expected);
  },
);
