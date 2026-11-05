import { Activity } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AnswerActions } from "./AnswerActions";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it("keeps Copy confirmation transient after its chat sleeps and wakes", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("navigator", {
    clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
  const view = (mode: "visible" | "hidden") => (
    <Activity mode={mode}>
      <AnswerActions text="A durable answer" />
    </Activity>
  );
  const { rerender } = render(view("visible"));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
  });
  expect(screen.getByRole("button", { name: "Copied" })).toBeVisible();
  rerender(view("hidden"));
  act(() => vi.advanceTimersByTime(2_000));
  rerender(view("visible"));
  act(() => vi.advanceTimersByTime(0));
  expect(screen.getByRole("button", { name: "Copy" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Copied" })).toBeNull();
});

it("renews the transient check for a second successful copy", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("navigator", {
    clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
  render(<AnswerActions text="A durable answer" />);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
  });
  act(() => vi.advanceTimersByTime(500));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Copied" }));
  });
  act(() => vi.advanceTimersByTime(800));
  expect(screen.getByRole("button", { name: "Copied" })).toBeVisible();
  act(() => vi.advanceTimersByTime(500));
  expect(screen.getByRole("button", { name: "Copy" })).toBeVisible();
});
