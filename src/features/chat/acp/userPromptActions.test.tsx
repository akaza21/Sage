import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ItemView } from "./TurnActivityItem";
import { textItem } from "./acpProjection.fixture";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
it("copies a user's original multiline Markdown prompt and retains its saved time", async () => {
  let clipboard = "";
  vi.stubGlobal("navigator", {
    clipboard: {
      writeText: async (text: string) => {
        clipboard = text;
      },
    },
  });
  const prompt = "**keep** `punctuation`\nsecond line";
  const { container } = render(
    <ItemView
      item={{ ...textItem(), payload: { role: "user", text: prompt } }}
      interactive
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Copy message" }));
  await screen.findByRole("button", { name: "Copied" });
  expect(clipboard).toBe(prompt);
  expect(container.querySelector(".chat-user-actions time")).toHaveAttribute(
    "datetime",
    "2026-08-31T09:05:00.000Z",
  );
});
