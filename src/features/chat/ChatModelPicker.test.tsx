import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ChatModelPicker } from "./ChatModelPicker";

afterEach(cleanup);

it("shows human names without changing the selected harness ID", () => {
  const onSelect = vi.fn();
  render(
    <ChatModelPicker
      listboxId="models"
      models={[
        {
          id: "claude-sonnet-4-5-20250929",
          name: "Claude Sonnet 4 5 20250929",
        },
        { id: "gpt-5.4-mini", name: "gpt-5.4-mini" },
        { id: "custom/model:fast", name: "My private model" },
      ]}
      selectedId="claude-sonnet-4-5-20250929"
      disabled={false}
      onSelect={onSelect}
    />,
  );
  expect(
    screen.getByRole("option", { name: "Claude Sonnet 4.5" }),
  ).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("option", { name: "GPT-5.4 Mini" }));
  expect(onSelect).toHaveBeenCalledExactlyOnceWith("gpt-5.4-mini");
  expect(
    screen.getByRole("option", { name: "My private model" }),
  ).toBeVisible();
});

it("preserves the harness model groups", () => {
  render(
    <ChatModelPicker
      listboxId="models"
      models={[
        { id: "fast", name: "Fast", group: "Everyday" },
        { id: "deep", name: "Deep", group: "Reasoning" },
      ]}
      selectedId="deep"
      disabled={false}
      onSelect={vi.fn()}
    />,
  );
  expect(
    within(screen.getByRole("group", { name: "Everyday" })).getByRole(
      "option",
      { name: "Fast" },
    ),
  ).toBeVisible();
  expect(
    within(screen.getByRole("group", { name: "Reasoning" })).getByRole(
      "option",
      { name: "Deep" },
    ),
  ).toHaveAttribute("aria-selected", "true");
});

it("visibly distinguishes colliding human names across groups by exact harness ID", () => {
  const onSelect = vi.fn();
  render(
    <ChatModelPicker
      listboxId="models"
      models={[
        {
          id: "claude-sonnet-4-20250514",
          name: "claude-sonnet-4-20250514",
          group: "Recommended",
        },
        {
          id: "claude-sonnet-4-20250601",
          name: "Claude Sonnet 4 20250601",
          group: "Other models",
        },
        { id: "private/sonnet", name: "Claude Sonnet 4" },
        { id: "gpt-5.4-mini", name: "gpt-5.4-mini" },
      ]}
      selectedId="claude-sonnet-4-20250514"
      disabled={false}
      onSelect={onSelect}
    />,
  );
  const selected = screen.getByRole("option", {
    name: "Claude Sonnet 4 (claude-sonnet-4-20250514)",
  });
  expect(selected).toHaveAttribute("aria-selected", "true");
  expect(within(selected).getByText("claude-sonnet-4-20250514")).toBeVisible();
  const snapshot = screen.getByRole("option", {
    name: "Claude Sonnet 4 (claude-sonnet-4-20250601)",
  });
  expect(within(snapshot).getByText("claude-sonnet-4-20250601")).toBeVisible();
  fireEvent.click(snapshot);
  fireEvent.click(
    screen.getByRole("option", { name: "Claude Sonnet 4 (private/sonnet)" }),
  );
  expect(onSelect.mock.calls).toEqual([
    ["claude-sonnet-4-20250601"],
    ["private/sonnet"],
  ]);
  const unique = screen.getByRole("option", { name: "GPT-5.4 Mini" });
  expect(unique).not.toHaveTextContent("gpt-5.4-mini");
});

it("uses default metadata for recommendations and caps visible models", () => {
  render(
    <ChatModelPicker
      listboxId="models"
      models={Array.from({ length: 205 }, (_, index) => ({
        id: "model-" + index,
        name: "Model " + index,
        isDefault: index === 0,
      }))}
      selectedId="model-0"
      disabled={false}
      onSelect={vi.fn()}
    />,
  );
  expect(
    within(screen.getByRole("group", { name: "Recommended" })).getAllByRole(
      "option",
    ),
  ).toHaveLength(1);
  expect(screen.getAllByRole("option")).toHaveLength(200);
  expect(screen.getByRole("status")).toHaveTextContent(
    "Showing first 200 models",
  );
});
