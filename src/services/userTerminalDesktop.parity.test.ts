import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, it } from "vitest";

import {
  TERMINAL_INPUT_BYTES,
  TERMINAL_MAX_COLUMNS,
  TERMINAL_MAX_PER_CHAT,
  TERMINAL_MAX_ROWS,
} from "./userTerminalDesktop";

const rust = readFileSync(
  path.resolve(__dirname, "../../src-tauri/src/user_terminal.rs"),
  "utf8",
);

function rustConstant(name: string): number {
  const value = new RegExp(`pub const ${name}: \\w+ = ([\\d_]+);`, "u").exec(
    rust,
  )?.[1];
  if (!value) throw new Error(`${name} not found in user_terminal.rs`);
  return Number(value.replaceAll("_", ""));
}

it("mirrors the native terminal limits", () => {
  expect({
    columns: TERMINAL_MAX_COLUMNS,
    rows: TERMINAL_MAX_ROWS,
    input: TERMINAL_INPUT_BYTES,
    perChat: TERMINAL_MAX_PER_CHAT,
  }).toEqual({
    columns: rustConstant("MAX_COLUMNS"),
    rows: rustConstant("MAX_ROWS"),
    input: rustConstant("INPUT_BYTES"),
    perChat: rustConstant("MAX_PER_CHAT"),
  });
});
