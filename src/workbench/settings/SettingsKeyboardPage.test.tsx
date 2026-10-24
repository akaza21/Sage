import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { SettingsKeyboardPage } from "./SettingsKeyboardPage";

afterEach(cleanup);

it("names the Agent chat shortcut group with a valid heading reference", () => {
  render(<SettingsKeyboardPage visible={new Set(["chat.findInChat"])} />);
  expect(screen.getByRole("group", { name: "Agent chat" })).toHaveTextContent(
    "Find in chat",
  );
});
