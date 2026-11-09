import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const { listNativeSessions } = vi.hoisted(() => ({
  listNativeSessions: vi.fn(),
}));
vi.mock("../services/nativeSessionImport", () => ({
  listNativeSessions,
  importNativeSession: vi.fn(),
}));

import { ImportSessionDialog } from "./ImportSessionDialog";

afterEach(cleanup);

it("closes while the listing is still busy and ignores its late result", async () => {
  let answer: (page: unknown) => void = () => undefined;
  listNativeSessions.mockReturnValueOnce(
    new Promise((resolve) => (answer = resolve)),
  );
  const errors = vi.spyOn(console, "error");
  render(<ImportSessionDialog />);
  const dialog = screen.getByRole("dialog", { hidden: true });
  expect(dialog.querySelector("[aria-busy=true]")).not.toBeNull();
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(screen.queryByRole("dialog", { hidden: true })).toBeNull();
  await act(async () =>
    answer({
      sessions: [{ threadId: "thread-1", title: "Late", updatedAt: 1 }],
      nextCursor: null,
      lister: { sessionId: "session-1", runtimeEpoch: "1" },
    }),
  );
  expect(screen.queryByText("Late")).toBeNull();
  expect(errors).not.toHaveBeenCalled();
});
