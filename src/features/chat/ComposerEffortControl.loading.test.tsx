import { useRef, useState, type ReactElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerEffortControl } from "./ComposerEffortControl";

const loading = vi.hoisted(() => {
  let resolve!: (module: { default: () => ReactElement }) => void;
  const promise = new Promise<{ default: () => ReactElement }>((done) => {
    resolve = done;
  });
  return { promise, resolve };
});
vi.mock("./ComposerEffortMenu", () => loading.promise);
afterEach(cleanup);

it("returns Escape to Add while its effort menu is still loading", async () => {
  const loaded = vi.fn(() => <button autoFocus>Loaded thinking levels</button>);
  function Fixture() {
    const anchorRef = useRef<HTMLButtonElement>(null);
    const [open, setOpen] = useState(false);
    return (
      <>
        <button ref={anchorRef} onClick={() => setOpen(true)}>
          Add
        </button>
        <textarea aria-label="Message" />
        <ComposerEffortControl
          anchorRef={anchorRef}
          fallbackRef={anchorRef}
          option={{
            id: "effort",
            name: "Thinking level",
            type: "select",
            currentValue: "high",
            options: [{ value: "high", name: "High" }],
          }}
          modelLabel="Actual model"
          open={open}
          onToggle={() => setOpen(!open)}
          onDismiss={() => setOpen(false)}
          onOpenModel={() => {}}
          modelAvailable
          disabled={false}
          pending={false}
          onSetOption={async () => {}}
        />
      </>
    );
  }
  render(<Fixture />);
  const add = screen.getByRole("button", { name: "Add" });
  fireEvent.click(add);
  expect(screen.getByRole("status")).toBeTruthy();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(add).toHaveFocus();
  const prompt = screen.getByRole("textbox", { name: "Message" });
  prompt.focus();
  await act(async () => loading.resolve({ default: loaded }));
  expect(loaded).not.toHaveBeenCalled();
  expect(prompt).toHaveFocus();
});
