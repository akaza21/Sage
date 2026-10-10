import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  SearchablePicker,
  type SearchablePickerOption,
} from "./SearchablePicker";

afterEach(() => cleanup());

const options: readonly SearchablePickerOption[] = [
  { id: "one", label: "Root A — Repository root" },
  { id: "two", label: "Root B — packages/tool" },
  {
    id: "three",
    label: "Unavailable repository",
    disabledReason: "Repository is unavailable.",
  },
];

function Harness({
  candidates = options,
  disabled = false,
  state,
}: {
  candidates?: readonly SearchablePickerOption[];
  disabled?: boolean;
  state?: "error" | "idle" | "loading";
}) {
  const [selectedId, setSelectedId] = useState("one");
  return (
    <SearchablePicker
      ariaLabel="Select repository"
      candidates={candidates}
      disabled={disabled}
      onSelect={setSelectedId}
      selectedId={selectedId}
      state={state}
      stateTitle="Repository status"
    />
  );
}

describe("SearchablePicker", () => {
  it("does not leave a pointer-active row behind keyboard focus in a non-searchable list", () => {
    render(
      <SearchablePicker
        ariaLabel="Harness"
        candidates={options}
        searchable={false}
        selectedId="one"
        onSelect={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    const first = screen.getAllByRole("option")[0];
    const second = screen.getAllByRole("option")[1];
    fireEvent.mouseMove(first);
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(second).toHaveFocus();
    expect(first).not.toHaveAttribute("data-active");
  });

  it("focuses a newly available harness option when discovery completes with its list open", () => {
    const onSelect = vi.fn();
    const view = render(
      <SearchablePicker
        ariaLabel="Harness"
        candidates={[
          {
            id: "one",
            label: "First harness",
            disabledReason: "Detecting harnesses",
          },
        ]}
        searchable={false}
        selectedId="one"
        state="loading"
        onSelect={onSelect}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Harness" });
    trigger.focus();
    fireEvent.click(trigger);
    expect(trigger).toHaveFocus();
    view.rerender(
      <SearchablePicker
        ariaLabel="Harness"
        candidates={[
          { id: "one", label: "First harness" },
          { id: "two", label: "Second harness" },
        ]}
        searchable={false}
        selectedId="one"
        onSelect={onSelect}
      />,
    );
    const first = screen.getByRole("option", { name: "First harness" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: "s" });
    const second = screen.getByRole("option", { name: "Second harness" });
    expect(second).toHaveFocus();
    fireEvent.click(second);
    expect(onSelect).toHaveBeenCalledExactlyOnceWith("two");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("returns Tab from non-searchable options and dismisses an all-disabled list from its trigger", () => {
    const view = render(
      <SearchablePicker
        ariaLabel="Harness"
        candidates={options}
        searchable={false}
        selectedId="one"
        onSelect={vi.fn()}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Harness" });
    fireEvent.click(trigger);
    const first = screen.getByRole("option", {
      name: "Root A — Repository root",
    });
    expect(first).toHaveFocus();
    expect(fireEvent.keyDown(first, { key: "Tab" })).toBe(true);
    expect(trigger).toHaveFocus();
    expect(screen.queryByRole("listbox")).toBeNull();
    view.rerender(
      <SearchablePicker
        ariaLabel="Harness"
        candidates={[options[2]]}
        searchable={false}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );
    fireEvent.click(trigger);
    expect(screen.getByRole("listbox")).toBeVisible();
    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("keeps its editable combobox focused while a sibling listbox owns active-descendant options", () => {
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "Select repository" });
    fireEvent.click(trigger);
    const input = screen.getByRole("combobox", { name: "Select repository" });
    const listbox = screen.getByRole("listbox", { name: "Select repository" });
    expect(input).toHaveFocus();
    expect(input).toHaveAttribute("aria-autocomplete", "list");
    expect(input).toHaveAttribute("aria-controls", listbox.id);
    expect(listbox.contains(input)).toBe(false);
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(input).toHaveFocus();
    expect(input).toHaveAttribute("aria-activedescendant");
    expect(input.getAttribute("aria-activedescendant")).toContain("one");
  });

  it("filters labels, preserves ordinary editing, skips disabled options, pages ten rows, and dispatches once", () => {
    const onSelect = vi.fn();
    render(
      <SearchablePicker
        ariaLabel="Picker"
        candidates={options}
        onSelect={onSelect}
        selectedId="one"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Picker" }));
    const input = screen.getByRole("combobox", { name: "Picker" });
    fireEvent.change(input, { target: { value: "root b" } });
    expect(input).toHaveValue("root b");
    fireEvent.keyDown(input, { key: "End" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith("two");
  });

  it("uses a concise trigger label without losing the full menu label", () => {
    render(
      <SearchablePicker
        ariaLabel="Harness"
        candidates={[
          {
            id: "claude",
            label: "Claude Code — 2.1.270 — available",
            triggerLabel: "Claude Code",
          },
        ]}
        onSelect={vi.fn()}
        selectedId="claude"
      />,
    );

    expect(screen.getByRole("button", { name: "Harness" })).toHaveTextContent(
      "Claude Code",
    );
    expect(
      screen
        .getByRole("button", { name: "Harness" })
        .querySelector(".searchable-picker__chevron"),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    expect(
      screen.getByRole("option", {
        name: "Claude Code — 2.1.270 — available",
      }),
    ).toBeVisible();
  });

  it("moves every named navigation key across enabled rows, wraps, pages, and retains current value while filtered", async () => {
    vi.useFakeTimers();
    try {
      const candidates = Array.from({ length: 15 }, (_, index) => ({
        id: `item-${index}`,
        label: `Item ${index}`,
        ...(index === 1 ? { disabledReason: "Unavailable" } : {}),
      }));
      const onSelect = vi.fn();
      const scrollIntoView = vi.fn();
      Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
        configurable: true,
        value: scrollIntoView,
      });
      render(
        <SearchablePicker
          ariaLabel="Keyboard picker"
          candidates={candidates}
          onSelect={onSelect}
          selectedId="item-0"
        />,
      );
      fireEvent.click(screen.getByRole("button", { name: "Keyboard picker" }));
      const input = screen.getByRole("combobox", {
        name: "Keyboard picker",
      });
      const active = () => input.getAttribute("aria-activedescendant");
      fireEvent.keyDown(input, { key: "ArrowDown" });
      expect(active()).toContain("item-0");
      fireEvent.keyDown(input, { key: "ArrowDown" });
      expect(active()).toContain("item-2");
      fireEvent.keyDown(input, { key: "ArrowUp" });
      expect(active()).toContain("item-0");
      fireEvent.keyDown(input, { key: "ArrowUp" });
      expect(active()).toContain("item-14");
      fireEvent.keyDown(input, { key: "Home" });
      expect(active()).toContain("item-0");
      fireEvent.keyDown(input, { key: "End" });
      expect(active()).toContain("item-14");
      fireEvent.keyDown(input, { key: "PageUp" });
      expect(active()).toContain("item-4");
      fireEvent.keyDown(input, { key: "PageDown" });
      expect(active()).toContain("item-14");
      expect(scrollIntoView).toHaveBeenCalled();
      fireEvent.change(input, { target: { value: "Item 14" } });
      await act(() => vi.advanceTimersByTime(50));
      expect(screen.getByText("Current: Item 0")).toBeVisible();
      fireEvent.keyDown(input, { key: "End" });
      fireEvent.keyDown(input, { key: "Enter" });
      expect(onSelect).toHaveBeenCalledExactlyOnceWith("item-14");
    } finally {
      vi.useRealTimers();
      Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
    }
  });

  it("fails closed above 500 candidates and renders at most 200 matches with a refine row", async () => {
    const many = Array.from({ length: 501 }, (_, index) => ({
      id: String(index),
      label: `Repository ${index}`,
    }));
    const exact = many.slice(0, 500);
    const { rerender } = render(<Harness candidates={many} />);
    expect(screen.getByRole("alert")).toHaveTextContent("500");
    rerender(<Harness candidates={exact} />);
    fireEvent.click(screen.getByRole("button", { name: "Select repository" }));
    expect(screen.getAllByRole("option")).toHaveLength(200);
    expect(screen.getByText("300 more — refine")).toBeVisible();
    const firstId = screen.getAllByRole("option")[0]?.id;
    rerender(<Harness candidates={exact} />);
    expect(screen.getAllByRole("option")[0]?.id).toBe(firstId);
    const exactlyTwoHundred = exact.slice(0, 200);
    rerender(<Harness candidates={exactlyTwoHundred} />);
    expect(screen.getAllByRole("option")).toHaveLength(200);
    expect(screen.queryByText(/more — refine/)).toBeNull();
    const twoHundredOne = exact.slice(0, 201);
    rerender(<Harness candidates={twoHundredOne} />);
    expect(screen.getAllByRole("option")).toHaveLength(200);
    expect(screen.getByText("1 more — refine")).toBeVisible();
    rerender(<Harness candidates={many} />);
    await act(() => Promise.resolve());
    rerender(<Harness candidates={exact} />);
    expect(
      screen.queryByRole("combobox", { name: "Select repository" }),
    ).toBeNull();
  });

  it("debounces the visible filter no longer than 50ms and cancels superseded queries", async () => {
    vi.useFakeTimers();
    try {
      render(<Harness />);
      fireEvent.click(
        screen.getByRole("button", { name: "Select repository" }),
      );
      const input = screen.getByRole("combobox", {
        name: "Select repository",
      });
      fireEvent.change(input, { target: { value: "root b" } });
      fireEvent.change(input, { target: { value: "unavailable" } });
      await act(() => vi.advanceTimersByTime(49));
      expect(screen.getByRole("option", { name: /Root A/ })).toBeVisible();
      await act(() => vi.advanceTimersByTime(1));
      expect(
        screen.getByRole("option", { name: /Unavailable repository/ }),
      ).toBeVisible();
      expect(screen.queryByRole("option", { name: /Root B/ })).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("cancels pending debounce work when the picker closes or unmounts", async () => {
    vi.useFakeTimers();
    try {
      const { unmount } = render(<Harness />);
      const trigger = screen.getByRole("button", {
        name: "Select repository",
      });
      fireEvent.click(trigger);
      fireEvent.change(
        screen.getByRole("combobox", { name: "Select repository" }),
        { target: { value: "unavailable" } },
      );
      fireEvent.keyDown(
        screen.getByRole("combobox", { name: "Select repository" }),
        { key: "Escape" },
      );
      await act(() => vi.advanceTimersByTime(50));
      fireEvent.click(trigger);
      expect(screen.getByRole("option", { name: /Root A/ })).toBeVisible();
      const input = screen.getByRole("combobox", {
        name: "Select repository",
      });
      fireEvent.keyDown(input, { key: "ArrowDown" });
      expect(input).toHaveAttribute("aria-activedescendant");
      fireEvent.click(trigger);
      fireEvent.click(trigger);
      expect(
        screen.getByRole("combobox", { name: "Select repository" }),
      ).not.toHaveAttribute("aria-activedescendant");
      unmount();
      await act(() => vi.runOnlyPendingTimers());
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps unaffected option rows memoized across active navigation", () => {
    const onOptionRender = vi.fn();
    render(
      <SearchablePicker
        ariaLabel="Picker"
        candidates={options}
        onOptionRender={onOptionRender}
        onSelect={vi.fn()}
        selectedId="one"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Picker" }));
    onOptionRender.mockClear();
    fireEvent.keyDown(screen.getByRole("combobox", { name: "Picker" }), {
      key: "ArrowDown",
    });
    expect(onOptionRender).toHaveBeenCalledTimes(1);
    expect(onOptionRender).toHaveBeenCalledWith("one");
  });

  it("fails closed when an open picker becomes disabled", async () => {
    const onSelect = vi.fn();
    const { rerender } = render(
      <SearchablePicker
        ariaLabel="Picker"
        candidates={options}
        onSelect={onSelect}
        selectedId="one"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Picker" }));
    const input = screen.getByRole("combobox", { name: "Picker" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    const retainedRow = screen.getByRole("option", {
      name: "Root B — packages/tool",
    });
    expect(retainedRow).toHaveAttribute("data-active", "true");
    rerender(
      <SearchablePicker
        ariaLabel="Picker"
        candidates={options}
        disabled
        onSelect={onSelect}
        selectedId="one"
      />,
    );
    fireEvent.click(retainedRow);
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.queryByRole("combobox", { name: "Picker" })).toBeNull();
    expect(document.querySelector("[data-active='true']")).toBeNull();
    await act(() => Promise.resolve());
    rerender(
      <SearchablePicker
        ariaLabel="Picker"
        candidates={options}
        onSelect={onSelect}
        selectedId="one"
      />,
    );
    expect(screen.queryByRole("combobox", { name: "Picker" })).toBeNull();
  });

  it("shows selected value, disabled reason, and supplied state notices without making disabled rows selectable", () => {
    const { rerender } = render(<Harness state="loading" />);
    fireEvent.click(screen.getByRole("button", { name: "Select repository" }));
    expect(screen.getByText("Current: Root A — Repository root")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("Repository status");
    const disabled = screen.getByRole("option", {
      name: /Unavailable repository/,
    });
    expect(disabled).toHaveAttribute("aria-disabled", "true");
    expect(disabled).toHaveAccessibleName(
      "Unavailable repositoryRepository is unavailable.",
    );
    expect(disabled).toHaveAccessibleDescription("Repository is unavailable.");
    const longLabel =
      "A repository label long enough to require clipped current-value chrome ".repeat(
        3,
      );
    rerender(
      <SearchablePicker
        ariaLabel="Select repository"
        candidates={[{ id: "one", label: longLabel }]}
        onSelect={vi.fn()}
        selectedId="one"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Select repository" }));
    expect(
      document.querySelector(".searchable-picker__current"),
    ).toHaveTextContent(`Current: ${longLabel.trim()}`);
  });

  it("uses the existing idle notice for an empty filtered result", async () => {
    vi.useFakeTimers();
    try {
      render(<Harness />);
      fireEvent.click(
        screen.getByRole("button", { name: "Select repository" }),
      );
      fireEvent.change(
        screen.getByRole("combobox", { name: "Select repository" }),
        { target: { value: "missing" } },
      );
      await act(() => vi.advanceTimersByTime(50));
      expect(screen.getByRole("status")).toHaveTextContent(
        "No select repository found.",
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("restores trigger focus on Escape, anchors Tab to the trigger without routing it, and preserves outside targets", () => {
    render(
      <>
        <Harness />
        <button type="button">Outside</button>
      </>,
    );
    const trigger = screen.getByRole("button", { name: "Select repository" });
    fireEvent.click(trigger);
    const input = screen.getByRole("combobox", { name: "Select repository" });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    const outside = screen.getByRole("button", { name: "Outside" });
    const tab = createEvent.keyDown(
      screen.getByRole("combobox", { name: "Select repository" }),
      { key: "Tab" },
    );
    fireEvent(screen.getByRole("combobox", { name: "Select repository" }), tab);
    expect(tab.defaultPrevented).toBe(false);
    expect(trigger).toHaveFocus();
    fireEvent.click(trigger);
    outside.focus();
    fireEvent.pointerDown(outside);
    expect(outside).toHaveFocus();
  });

  it("consumes one Escape in a document portal before the compact parent can close", () => {
    const closeParent = vi.fn();
    function CompactHarness() {
      const [open, setOpen] = useState(true);
      return open ? (
        <section
          role="dialog"
          aria-label="Compact project tools"
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            closeParent();
            setOpen(false);
          }}
        >
          <Harness />
        </section>
      ) : null;
    }
    render(<CompactHarness />);
    const parent = screen.getByRole("dialog", {
      name: "Compact project tools",
    });
    const trigger = screen.getByRole("button", { name: "Select repository" });
    fireEvent.click(trigger);
    const input = screen.getByRole("combobox", { name: "Select repository" });
    expect(parent.contains(input)).toBe(false);
    fireEvent.keyDown(input, { key: "Escape" });
    expect(closeParent).not.toHaveBeenCalled();
    expect(parent).toBeVisible();
    expect(trigger).toHaveFocus();
    expect(screen.queryByRole("combobox")).toBeNull();
    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(closeParent).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("keeps a modal picker portal-owned, contains Escape, and leaves Tab to the dialog", () => {
    function ModalHarness() {
      const dialogRef = useRef<HTMLDialogElement>(null);
      return (
        <dialog ref={dialogRef} open>
          <SearchablePicker
            ariaLabel="Target"
            candidates={options}
            onSelect={vi.fn()}
            portalTarget="closest-dialog"
            selectedId="one"
          />
        </dialog>
      );
    }
    render(<ModalHarness />);
    const dialog = screen.getByRole("dialog");
    const trigger = screen.getByRole("button", { name: "Target" });
    fireEvent.click(trigger);
    const input = screen.getByRole("combobox", { name: "Target" });
    expect(dialog.contains(input)).toBe(true);
    const escape = createEvent.keyDown(input, { key: "Escape" });
    fireEvent(input, escape);
    expect(escape.defaultPrevented).toBe(true);
    expect(trigger).toHaveFocus();
    expect(dialog).toBeVisible();
    fireEvent.click(trigger);
    const tabInput = screen.getByRole("combobox", { name: "Target" });
    const tab = createEvent.keyDown(tabInput, { key: "Tab" });
    fireEvent(tabInput, tab);
    expect(tab.defaultPrevented).toBe(false);
    expect(trigger).toHaveFocus();
    expect(screen.queryByRole("combobox", { name: "Target" })).toBeNull();
  });
});
