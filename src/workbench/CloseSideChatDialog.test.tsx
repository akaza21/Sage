import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { CloseSideChatDialog, SideChatHome } from "./SideChatChrome";

afterEach(cleanup);

it("uses the 24px empty side-chat icon (R3-12b)", () => {
  const { container } = render(<SideChatHome />);
  expect(container.querySelector("svg")).toHaveAttribute("width", "24");
  expect(container.querySelector("svg")).toHaveAttribute("height", "24");
});

it("focuses the destructive confirmation and sends the persisted preference intent", async () => {
  const close = vi.fn().mockResolvedValue(undefined);
  const cancel = vi.fn();
  render(<CloseSideChatDialog onCancel={cancel} onClose={close} />);
  expect(
    screen.getByRole("dialog", { name: "Close side chat?" }),
  ).toHaveTextContent(
    "This side chat will be gone and can't be recovered. Are you sure?",
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Close side chat" }),
    ).toHaveFocus(),
  );
  expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible();
  fireEvent.click(screen.getByRole("checkbox", { name: "Don't ask again" }));
  fireEvent.click(screen.getByRole("button", { name: "Close side chat" }));
  await waitFor(() => expect(close).toHaveBeenCalledWith(true));
});

it("cancels on Escape without accepting the checkbox", () => {
  const cancel = vi.fn();
  const close = vi.fn();
  render(<CloseSideChatDialog onCancel={cancel} onClose={close} />);
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(cancel).toHaveBeenCalled();
  expect(close).not.toHaveBeenCalled();
});

it("keeps the dialog open with a visible error when closing fails", async () => {
  const close = vi
    .fn()
    .mockRejectedValue(new Error("Side chat shutdown is unsettled"));
  render(<CloseSideChatDialog onCancel={vi.fn()} onClose={close} />);
  expect(
    screen.getByRole("checkbox", { name: "Don't ask again" }),
  ).not.toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "Close side chat" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Side chat shutdown is unsettled",
  );
  expect(close).toHaveBeenCalledWith(false);
  expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled();
});
