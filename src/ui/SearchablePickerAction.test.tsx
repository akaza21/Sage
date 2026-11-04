import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import { SearchablePicker } from "./SearchablePicker";

it("renders a row action beside its option without selecting the option", () => {
  const onSelect = vi.fn();
  const onAction = vi.fn();
  render(
    <SearchablePicker
      ariaLabel="Harness"
      open
      searchable={false}
      candidates={[
        {
          id: "claude-code",
          label: "Claude Code",
          subtitle: "Signed out",
          action: {
            label: "Sign in",
            accessibleLabel: "Sign in to Claude Code",
            onAction,
          },
        },
        { id: "codex", label: "Codex" },
      ]}
      onSelect={onSelect}
      selectedId="codex"
    />,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Sign in to Claude Code" }),
  );
  expect(onAction).toHaveBeenCalledTimes(1);
  expect(onSelect).not.toHaveBeenCalled();
  expect(screen.getByText("Signed out")).toBeTruthy();
  expect(screen.getAllByRole("option")).toHaveLength(2);
});
