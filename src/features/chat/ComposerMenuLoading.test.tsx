import { lazy, Suspense, useEffect, useRef, useState } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  act,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ComposerMenuLoading } from "./ComposerMenuLoading";

afterEach(cleanup);

it.each(["Escape", "Tab", "outside"])(
  "cancels a delayed menu on %s without reopening or stealing focus",
  async (gesture) => {
    const mounted = vi.fn();
    let resolveMenu!: (module: { default: typeof Menu }) => void;
    function Menu() {
      useEffect(mounted, []);
      return <button autoFocus>Loaded option</button>;
    }
    const DelayedMenu = lazy(
      () =>
        new Promise<{ default: typeof Menu }>((resolve) => {
          resolveMenu = resolve;
        }),
    );
    function Fixture() {
      const triggerRef = useRef<HTMLButtonElement>(null);
      const [open, setOpen] = useState(false);
      return (
        <>
          <button ref={triggerRef} onClick={() => setOpen(!open)}>
            Model
          </button>
          <textarea aria-label="Message" />
          {open ? (
            <Suspense
              fallback={
                <ComposerMenuLoading
                  triggerRef={triggerRef}
                  onDismiss={() => setOpen(false)}
                />
              }
            >
              <DelayedMenu />
            </Suspense>
          ) : null}
        </>
      );
    }
    render(<Fixture />);
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    expect(screen.getByRole("status")).toBeTruthy();
    const prompt = screen.getByRole("textbox", { name: "Message" });
    if (gesture === "outside") fireEvent.pointerDown(prompt);
    else fireEvent.keyDown(document, { key: gesture });
    expect(screen.queryByRole("status")).toBeNull();
    if (gesture === "Escape")
      expect(screen.getByRole("button", { name: "Model" })).toHaveFocus();
    prompt.focus();
    await act(async () => resolveMenu({ default: Menu }));
    expect(screen.queryByRole("button", { name: "Loaded option" })).toBeNull();
    expect(mounted).not.toHaveBeenCalled();
    expect(prompt).toHaveFocus();
  },
);
