import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { useHandoffConfirmation } from "./useHandoffConfirmation";

afterEach(cleanup);

function Confirmation({
  scope,
  result,
}: {
  scope: string;
  result: (value: boolean) => void;
}) {
  const confirmation = useHandoffConfirmation(scope);
  return (
    <>
      <button onClick={() => void confirmation.request().then(result)}>
        Retry
      </button>
      {confirmation.dialog}
    </>
  );
}

it.each(["Cancel", "Replace and retry"])(
  "requires the app dialog's %s decision",
  async (label) => {
    const result = vi.fn();
    render(<Confirmation scope="chat-a" result={result} />);
    screen.getByText("Retry").focus();
    fireEvent.click(screen.getByText("Retry"));
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "Prior execution may have occurred",
    );
    expect(result).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByText(label));
    });
    expect(result).toHaveBeenCalledWith(label === "Replace and retry");
    expect(screen.getByText("Retry")).toHaveFocus();
  },
);

it("dismisses a pending decision when its scope changes or unmounts", async () => {
  const result = vi.fn();
  const view = render(<Confirmation scope="chat-a" result={result} />);
  fireEvent.click(screen.getByText("Retry"));
  await act(async () => {
    view.rerender(<Confirmation scope="chat-b" result={result} />);
  });
  expect(result).toHaveBeenCalledWith(false);
  expect(screen.queryByRole("dialog")).toBeNull();
  result.mockClear();
  fireEvent.click(screen.getByText("Retry"));
  await act(async () => view.unmount());
  expect(result).toHaveBeenCalledWith(false);
});

it("declines delayed requests after scope changes or unmount", async () => {
  const requests: Array<() => Promise<boolean>> = [];
  const expose = (request: () => Promise<boolean>) => requests.push(request);
  function Delayed({ scope }: { scope: string }) {
    const confirmation = useHandoffConfirmation(scope);
    useEffect(() => {
      expose(confirmation.request);
    }, [confirmation.request]);
    return confirmation.dialog;
  }
  const view = render(<Delayed scope="chat-a" />);
  const oldRequest = requests.at(-1)!;
  view.rerender(<Delayed scope="chat-b" />);
  await expect(oldRequest()).resolves.toBe(false);
  view.unmount();
  await expect(requests.at(-1)!()).resolves.toBe(false);
});
