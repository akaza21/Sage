import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { SearchablePicker } from "./SearchablePicker";

afterEach(() => cleanup());

const options = [
  { id: "one", label: "Root A" },
  { id: "two", label: "Root B" },
];

// No preload here: the first open must still land, focused, once the list
// arrives, and the list must then keep its query across a close.
it("opens a list that is still loading, then keeps its query", async () => {
  const onSelect = vi.fn();
  render(
    <SearchablePicker
      ariaLabel="Select repository"
      candidates={options}
      onSelect={onSelect}
      selectedId="one"
    />,
  );
  const trigger = screen.getByRole("button", { name: "Select repository" });
  fireEvent.click(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  const input = await screen.findByRole("combobox", {
    name: "Select repository",
  });
  expect(input).toHaveFocus();
  fireEvent.change(input, { target: { value: "B" } });
  fireEvent.keyDown(input, { key: "Escape" });
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(trigger).toHaveFocus();
  fireEvent.click(trigger);
  expect(screen.getByRole("combobox")).toHaveValue("B");
  await waitFor(() =>
    expect(screen.queryByRole("option", { name: "Root A" })).toBeNull(),
  );
  fireEvent.click(screen.getByRole("option", { name: "Root B" }));
  expect(onSelect).toHaveBeenCalledWith("two");
});
