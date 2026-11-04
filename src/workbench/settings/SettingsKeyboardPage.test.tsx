import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { SettingsKeyboardPage } from "./SettingsKeyboardPage";

afterEach(cleanup);

it("names the Agent chat shortcut group with a valid heading reference", () => {
  render(
    <SettingsKeyboardPage
      scopes={["Agent chat"]}
      visible={new Set(["chat.findInChat"])}
    />,
  );
  expect(screen.getByRole("group", { name: "Agent chat" })).toHaveTextContent(
    "Find in chat",
  );
});

it("lists only the scopes it is given", () => {
  render(
    <SettingsKeyboardPage
      scopes={["Global", "Editor"]}
      visible={new Set(["openSettings", "save", "chat.findInChat"])}
    />,
  );
  expect(screen.getByRole("group", { name: "Global" })).toHaveTextContent(
    "Open settings",
  );
  expect(screen.getByRole("group", { name: "Editor" })).toBeVisible();
  expect(screen.queryByRole("group", { name: "Agent chat" })).toBeNull();
});
