import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { snapshotPage } from "../../../app/agentDiff.fixture";
import * as acpDesktop from "../../../services/acpDesktop";
import { useTurnWorkspaceChanges } from "./turnWorkspaceBridge";
import { resetTurnWorkspaceChangesForTest } from "./turnWorkspaceChanges";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  resetTurnWorkspaceChangesForTest();
});

it("does not render another chat's ledger while a new owner is loading", async () => {
  const page = snapshotPage();
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockResolvedValue({
      rootId: "root-a",
      tracked: true,
      truncated: false,
      changes: [{ relativePath: "previous.ts", operation: "created" }],
    });
  const hook = renderHook(
    ({ current }) => useTurnWorkspaceChanges(current, true),
    { initialProps: { current: page } },
  );
  await waitFor(() =>
    expect(hook.result.current.changes[0]?.relativePath).toBe("previous.ts"),
  );
  let complete!: (value: unknown) => void;
  load.mockImplementation(
    () =>
      new Promise((resolve) => {
        complete = resolve;
      }),
  );
  hook.rerender({ current: { ...page, chatId: "other-chat" as never } });
  expect(hook.result.current.changes).toEqual([]);
  expect(hook.result.current.incompleteTurnIds.has(page.turns[0].id)).toBe(
    false,
  );
  await waitFor(() => expect(complete).toBeDefined());
  await act(async () =>
    complete({
      rootId: "root-a",
      tracked: true,
      truncated: false,
      changes: [{ relativePath: "current.ts", operation: "deleted" }],
    }),
  );
  await waitFor(() =>
    expect(hook.result.current.changes[0]?.relativePath).toBe("current.ts"),
  );
});

it("retains a stable ledger while newly terminal turns load", async () => {
  const page = snapshotPage();
  const load = vi
    .spyOn(acpDesktop, "getTurnWorkspaceChanges")
    .mockResolvedValue({
      rootId: "root-a",
      tracked: true,
      truncated: false,
      changes: [{ relativePath: "deleted.ts", operation: "deleted" }],
    });
  const hook = renderHook(
    ({ current }) => useTurnWorkspaceChanges(current, true),
    {
      initialProps: { current: page },
    },
  );
  await waitFor(() => expect(load).toHaveBeenCalledOnce());
  await waitFor(() =>
    expect(hook.result.current.incompleteTurnIds.size).toBe(0),
  );
  const retained = hook.result.current;
  load.mockImplementation(() => new Promise(() => undefined));
  hook.rerender({
    current: {
      ...page,
      turns: [
        ...page.turns,
        { ...page.turns[0], id: "new-turn" as never, ordinal: 2 },
      ],
    },
  });
  expect(hook.result.current).toBe(retained);
  expect(hook.result.current.incompleteTurnIds.has("new-turn")).toBe(false);
  expect(hook.result.current.changes).toEqual([
    {
      turnId: page.turns[0].id,
      rootId: "root-a",
      relativePath: "deleted.ts",
      operation: "deleted",
    },
  ]);
});
