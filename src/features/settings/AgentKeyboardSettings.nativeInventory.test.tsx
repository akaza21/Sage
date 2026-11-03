import { readFileSync } from "node:fs";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import AgentKeyboardSettings from "./AgentKeyboardSettings";

afterEach(cleanup);

it("covers every native browser shortcut action and its native-only aliases", () => {
  const native = readFileSync("src-tauri/src/browser/shortcuts.rs", "utf8");
  const labels = {
    FocusAddress: "Focus browser address",
    Reload: "Reload browser page",
    Back: "Browser back",
    Forward: "Browser forward",
    Find: "Find in browser page",
    ZoomIn: "Browser zoom in",
    ZoomOut: "Browser zoom out",
    ZoomReset: "Reset browser zoom",
    NewTab: "New browser tab",
    CloseTab: "Close browser tab",
    Stop: "Stop loading browser page",
    LeaveAnnotate: "Leave browser annotation",
  };
  const nativeActions = [
    ...new Set(
      [...native.matchAll(/Some\(BrowserShortcut::(\w+)\)/g)].map(
        (match) => match[1],
      ),
    ),
  ];
  expect(nativeActions.sort()).toEqual(Object.keys(labels).sort());
  render(<AgentKeyboardSettings />);
  for (const label of Object.values(labels))
    expect(screen.getByText(label)).toBeVisible();
  expect(native).toContain("0xBB | 0x6B => Some(BrowserShortcut::ZoomIn)");
  expect(native).toContain("0xBD | 0x6D => Some(BrowserShortcut::ZoomOut)");
  expect(
    screen.getByText("Native page zoom in").closest("tr"),
  ).toHaveTextContent("Ctrl+=");
  expect(
    screen.getByText("Native page zoom out").closest("tr"),
  ).toHaveTextContent("Ctrl+Subtract");
  expect(
    screen.getByText(/Esc leaves annotation mode first, even while idle/),
  ).toBeVisible();
});
