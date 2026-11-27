import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { createRef } from "react";
import { afterEach, expect, it, vi } from "vitest";

import { clearAcpProjectionRegistry } from "../../app/acpProjectionRegistry";
import { ComposerAttach } from "./ComposerAttach";

vi.mock("../../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  isTauriRuntime: () => true,
}));

afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
});

it("opens its menu where the press landed, and Files asks for the native pick", async () => {
  const onPick = vi.fn();
  const trigger = createRef<HTMLButtonElement>();
  render(
    <>
      <button ref={trigger} type="button">
        Attach files
      </button>
      <ComposerAttach
        chatId="chat-draft"
        presses={1}
        triggerRef={trigger}
        imageAction={{
          disabled: false,
          reason: null,
          accept: vi.fn(),
        }}
        onPick={onPick}
      />
    </>,
  );
  // Nothing renders again after the press, as in the app: the menu must be
  // placed (and so reachable) from its first commit.
  const item = screen.getByRole("menuitem", { name: /^Files/u });
  await act(async () => {
    fireEvent.click(item);
  });
  expect(onPick).toHaveBeenCalledOnce();
});
