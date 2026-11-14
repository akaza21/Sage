import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type * as NativeSessionImport from "../services/nativeSessionImport";

const { listNativeSessions, importNativeSession } = vi.hoisted(() => ({
  listNativeSessions: vi.fn(),
  importNativeSession: vi.fn(),
}));
vi.mock("../services/nativeSessionImport", async (importOriginal) => ({
  ...(await importOriginal<typeof NativeSessionImport>()),
  listNativeSessions,
  importNativeSession,
}));

import { useWorkbenchStore } from "../app/store";
import type { WorkbenchState } from "../app/store";
import { ImportSessionDialog } from "./ImportSessionDialog";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

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

it("lists only as many sessions as its runtime remembers, each still importable", async () => {
  const lister = { sessionId: "session-1", runtimeEpoch: "1" };
  let page = 0;
  listNativeSessions.mockImplementation(async () => {
    const index = page++;
    return {
      sessions: Array.from({ length: 25 }, (_, row) => ({
        threadId: `thread-${index}-${row}`,
        title: `Session ${index}-${row}`,
        updatedAt: 1,
      })),
      nextCursor: `cursor-${index + 1}`,
      lister,
    };
  });
  importNativeSession.mockRejectedValue(new Error("Refused"));
  useWorkbenchStore.setState({
    project: { id: "project-1" } as WorkbenchState["project"],
    projectTransitioning: false,
  });
  render(<ImportSessionDialog />);
  await screen.findByText("Session 0-0");
  for (;;) {
    const more = screen.queryByText("Load more");
    if (!more) break;
    await act(async () => fireEvent.click(more));
  }
  // Ten pages fit within the 256 sessions the runtime remembers.
  expect(listNativeSessions).toHaveBeenCalledTimes(10);
  expect(
    screen
      .getByRole("dialog", { hidden: true })
      .querySelectorAll(".chat-search-result"),
  ).toHaveLength(250);
  expect(
    screen.getByText("Only the most recently active sessions are listed."),
  ).not.toBeNull();
  await act(async () =>
    fireEvent.click(screen.getByText("Session 0-0").closest("button")!),
  );
  expect(importNativeSession).toHaveBeenCalledWith(
    expect.objectContaining({ threadId: "thread-0-0", lister }),
  );
});
