import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { StrictMode } from "react";

import { ModalDialog } from "./ModalDialog";

afterEach(cleanup);

describe("ModalDialog", () => {
  it("preserves explicit autofocus and caller focus-return policies in StrictMode", () => {
    render(<button type="button">Open modal</button>);
    const opener = screen.getByRole("button", { name: "Open modal" });
    opener.focus();
    const view = render(
      <StrictMode>
        <ModalDialog labelledBy="autofocus-title" onDismiss={vi.fn()}>
          <h2 id="autofocus-title">Compose</h2>
          <button type="button">Choose target</button>
          <textarea aria-label="Prompt" autoFocus />
        </ModalDialog>
      </StrictMode>,
    );
    expect(screen.getByRole("textbox", { name: "Prompt" })).toHaveFocus();
    opener.focus();
    render(<button type="button">New selection</button>);
    const selection = screen.getByRole("button", { name: "New selection" });
    selection.focus();
    view.unmount();
    expect(selection).toHaveFocus();
  });

  it("portals a named modal to the document body and cleans it up", () => {
    const view = render(
      <div data-testid="caller">
        <ModalDialog labelledBy="modal-title" onDismiss={vi.fn()}>
          <h2 id="modal-title">Shared modal</h2>
        </ModalDialog>
      </div>,
    );

    const dialog = screen.getByRole("dialog", { name: "Shared modal" });
    expect(dialog.parentElement).toBe(document.body);

    view.unmount();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("contains Tab focus for zero, one, and many eligible controls", () => {
    const { rerender } = render(
      <ModalDialog labelledBy="empty-title" onDismiss={vi.fn()}>
        <h2 id="empty-title">Empty modal</h2>
      </ModalDialog>,
    );
    let dialog = screen.getByRole("dialog", { name: "Empty modal" });
    expect(fireEvent.keyDown(dialog, { key: "Tab" })).toBe(false);

    rerender(
      <ModalDialog labelledBy="single-title" onDismiss={vi.fn()}>
        <h2 id="single-title">Single modal</h2>
        <button type="button">Only action</button>
      </ModalDialog>,
    );
    dialog = screen.getByRole("dialog", { name: "Single modal" });
    const only = screen.getByRole("button", { name: "Only action" });
    only.focus();
    expect(fireEvent.keyDown(dialog, { key: "Tab" })).toBe(false);
    expect(only).toHaveFocus();

    rerender(
      <ModalDialog labelledBy="many-title" onDismiss={vi.fn()}>
        <h2 id="many-title">Many modal</h2>
        <a href="#target">First link</a>
        <input aria-label="Hidden input" type="hidden" />
        <button hidden type="button">
          Hidden action
        </button>
        <button disabled type="button">
          Disabled action
        </button>
        <button tabIndex={-1} type="button">
          Untabbable action
        </button>
        <select aria-label="Middle select" defaultValue="one">
          <option value="one">One</option>
        </select>
        <textarea aria-label="Last field" />
      </ModalDialog>,
    );
    dialog = screen.getByRole("dialog", { name: "Many modal" });
    const first = screen.getByRole("link", { name: "First link" });
    const last = screen.getByRole("textbox", { name: "Last field" });

    last.focus();
    expect(fireEvent.keyDown(dialog, { key: "Tab" })).toBe(false);
    expect(first).toHaveFocus();

    first.focus();
    expect(fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true })).toBe(
      false,
    );
    expect(last).toHaveFocus();
  });

  it("routes Escape and native cancel through the caller dismissal policy", () => {
    const onDismiss = vi.fn();
    render(
      <ModalDialog labelledBy="dismiss-title" onDismiss={onDismiss}>
        <h2 id="dismiss-title">Dismiss modal</h2>
        <button type="button">Action</button>
      </ModalDialog>,
    );
    const dialog = screen.getByRole("dialog", { name: "Dismiss modal" });

    expect(fireEvent.keyDown(dialog, { key: "Escape" })).toBe(false);
    expect(onDismiss).toHaveBeenCalledTimes(1);

    expect(fireEvent(dialog, new Event("cancel", { cancelable: true }))).toBe(
      false,
    );
    expect(onDismiss).toHaveBeenCalledTimes(2);
  });
});
