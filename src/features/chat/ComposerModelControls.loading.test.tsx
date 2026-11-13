import { useEffect, useRef, useState, type ReactElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerModelControls } from "./ComposerModelControls";

const loading = vi.hoisted(() => {
  let resolve!: (module: { default: () => ReactElement }) => void;
  const promise = new Promise<{ default: () => ReactElement }>((done) => {
    resolve = done;
  });
  return { promise, resolve };
});
vi.mock("./ComposerModelMenu", () => loading.promise);
afterEach(cleanup);

it("cancels a cold model import without stealing focus and unmounts a closed menu", async () => {
  const mounted = vi.fn();
  const unmounted = vi.fn();
  function LoadedMenu() {
    useEffect(() => {
      mounted();
      return unmounted;
    }, []);
    return <button autoFocus>Loaded model</button>;
  }
  function Fixture() {
    const triggerRef = useRef<HTMLButtonElement>(null);
    const [open, setOpen] = useState(false);
    return (
      <>
        <textarea aria-label="Message" />
        <ComposerModelControls
          triggerRef={triggerRef}
          panelId="models"
          open={open}
          effortOpen={false}
          onToggle={() => setOpen(!open)}
          onToggleEffort={() => {}}
          onDismiss={() => setOpen(false)}
          choices={[
            { id: "actual", name: "Actual model" },
            { id: "other", name: "Other model" },
          ]}
          selectedId="actual"
          modelLabel="Actual model"
          effortLabel={null}
          contextWindow={null}
          effortOption={undefined}
          disabled={false}
          pending={false}
          onSelectModel={() => {}}
          onSetOption={async () => {}}
        />
      </>
    );
  }
  render(<Fixture />);
  const trigger = screen.getByRole("button", { name: "Model" });
  fireEvent.click(trigger);
  expect(screen.getByRole("status")).toBeInTheDocument();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(trigger).toHaveFocus();
  const prompt = screen.getByRole("textbox", { name: "Message" });
  prompt.focus();
  await act(async () => loading.resolve({ default: LoadedMenu }));
  expect(mounted).not.toHaveBeenCalled();
  expect(screen.queryByRole("status")).toBeNull();
  expect(prompt).toHaveFocus();
  fireEvent.click(trigger);
  expect(
    await screen.findByRole("button", { name: "Loaded model" }),
  ).toHaveFocus();
  expect(mounted).toHaveBeenCalledExactlyOnceWith();
  fireEvent.click(trigger);
  expect(screen.queryByRole("button", { name: "Loaded model" })).toBeNull();
  expect(unmounted).toHaveBeenCalledExactlyOnceWith();
});
