import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type * as PickerModule from "./SearchablePicker";
import type * as LoadModule from "./searchablePickerLoad";

// The open list's chunk arrives only when a test lets it, or fails on cue.
// Each test mocks it afresh, so each starts with the list not loaded.
const chunk = vi.hoisted(() => {
  let failures = 0;
  let release = () => {};
  let held = Promise.resolve();
  return {
    hold() {
      held = new Promise<void>((resolve) => {
        release = resolve;
      });
    },
    release: () => release(),
    failNext() {
      failures += 1;
    },
    async arrive() {
      await held;
      if (failures > 0) {
        failures -= 1;
        throw new Error("The list chunk failed to load.");
      }
    },
  };
});

let SearchablePicker: typeof PickerModule.SearchablePicker;
let preloadSearchablePicker: typeof LoadModule.preloadSearchablePicker;

beforeEach(async () => {
  vi.resetModules();
  chunk.hold();
  vi.doMock("./SearchablePickerPopover", async (importOriginal) => {
    await chunk.arrive();
    return importOriginal();
  });
  ({ SearchablePicker } = await import("./SearchablePicker"));
  ({ preloadSearchablePicker } = await import("./searchablePickerLoad"));
});
afterEach(() => {
  chunk.release();
  cleanup();
});

const options = [
  { id: "one", label: "Root A" },
  { id: "two", label: "Root B" },
];
const LABEL = "Select repository";

function renderPicker(searchable = true) {
  render(
    <>
      <SearchablePicker
        ariaLabel={LABEL}
        candidates={options}
        onSelect={() => undefined}
        searchable={searchable}
        selectedId="one"
      />
      <input aria-label="Elsewhere" />
    </>,
  );
  const trigger = screen.getByRole("button", { name: LABEL });
  trigger.focus();
  fireEvent.click(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  return trigger;
}

async function arrive() {
  chunk.release();
  await act(() => preloadSearchablePicker());
}

it("closes on Escape before its list has loaded, and the late list stays shut", async () => {
  const trigger = renderPicker();
  fireEvent.keyDown(trigger, { key: "Escape" });
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(trigger).toHaveFocus();
  await arrive();
  expect(screen.queryByRole("combobox")).toBeNull();
  expect(trigger).toHaveFocus();
});

it("names the list it controls only while that list is there", async () => {
  const trigger = renderPicker();
  expect(trigger).not.toHaveAttribute("aria-controls");
  await arrive();
  const listbox = screen.getByRole("listbox", { name: LABEL });
  expect(trigger).toHaveAttribute("aria-controls", listbox.id);
  fireEvent.click(trigger);
  expect(trigger).not.toHaveAttribute("aria-controls");
});

it("closes on Tab before its list has loaded", () => {
  const trigger = renderPicker();
  fireEvent.keyDown(trigger, { key: "Tab" });
  expect(trigger).toHaveAttribute("aria-expanded", "false");
});

it("closes on a press outside before its list has loaded", () => {
  const trigger = renderPicker();
  fireEvent.pointerDown(trigger);
  expect(trigger).toHaveAttribute("aria-expanded", "true");
  fireEvent.pointerDown(screen.getByRole("textbox", { name: "Elsewhere" }));
  expect(trigger).toHaveAttribute("aria-expanded", "false");
});

it("searches for what was typed on the trigger while the list loaded", async () => {
  const trigger = renderPicker();
  for (const key of ["R", "o", "o", "t", " ", "B"])
    fireEvent.keyDown(trigger, { key });
  fireEvent.keyDown(trigger, { key: "c", ctrlKey: true });
  await arrive();
  const input = screen.getByRole("combobox", { name: LABEL });
  expect(input).toHaveValue("Root B");
  expect(input).toHaveFocus();
  expect(screen.queryByRole("option", { name: "Root A" })).toBeNull();
  expect(screen.getByRole("option", { name: "Root B" })).toBeInTheDocument();
});

it("leaves focus where it moved while the list loaded", async () => {
  renderPicker();
  const elsewhere = screen.getByRole("textbox", { name: "Elsewhere" });
  elsewhere.focus();
  await arrive();
  expect(screen.getByRole("combobox", { name: LABEL })).toBeInTheDocument();
  expect(elsewhere).toHaveFocus();
});

it("opens a non-searchable list that arrives late without taking moved focus", async () => {
  const trigger = renderPicker(false);
  const elsewhere = screen.getByRole("textbox", { name: "Elsewhere" });
  elsewhere.focus();
  await arrive();
  expect(screen.getByRole("option", { name: "Root A" })).toBeInTheDocument();
  expect(elsewhere).toHaveFocus();
  fireEvent.keyDown(trigger, { key: "Escape" });
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(trigger);
  await waitFor(() =>
    expect(screen.getByRole("option", { name: "Root A" })).toHaveFocus(),
  );
});

it("closes with a notice when the list fails to load, then retries on the next open", async () => {
  chunk.failNext();
  const trigger = renderPicker();
  chunk.release();
  await waitFor(() =>
    expect(trigger).toHaveAttribute("aria-expanded", "false"),
  );
  expect(screen.getByRole("alert")).toHaveTextContent(
    `${LABEL} could not be loaded.`,
  );
  fireEvent.click(trigger);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(
    await screen.findByRole("combobox", { name: LABEL }),
  ).toBeInTheDocument();
  expect(trigger).toHaveAttribute("aria-expanded", "true");
});

it("keeps the typed query across the too-many-candidates notice", async () => {
  await arrive();
  const { SEARCHABLE_PICKER_MAX_CANDIDATES } =
    await import("./searchablePickerLoad");
  const tooMany = Array.from(
    { length: SEARCHABLE_PICKER_MAX_CANDIDATES + 1 },
    (_, index) => ({ id: `root-${index}`, label: `Root ${index}` }),
  );
  const picker = (candidates: typeof options) => (
    <SearchablePicker
      ariaLabel={LABEL}
      candidates={candidates}
      onSelect={() => undefined}
      selectedId="one"
    />
  );
  const { rerender } = render(picker(options));
  fireEvent.click(screen.getByRole("button", { name: LABEL }));
  fireEvent.change(screen.getByRole("combobox", { name: LABEL }), {
    target: { value: "B" },
  });
  rerender(picker(tooMany));
  expect(screen.getByRole("alert")).toHaveTextContent(
    `${LABEL} is unavailable`,
  );
  rerender(picker(options));
  const trigger = screen.getByRole("button", { name: LABEL });
  await waitFor(() =>
    expect(trigger).toHaveAttribute("aria-expanded", "false"),
  );
  fireEvent.click(trigger);
  expect(screen.getByRole("combobox", { name: LABEL })).toHaveValue("B");
});
