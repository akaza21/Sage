import { beforeEach, expect, it, vi } from "vitest";

const { invokeNative } = vi.hoisted(() => ({ invokeNative: vi.fn() }));
vi.mock("../ports/nativeInvoke", () => ({ invokeNative }));

import {
  closeUserTerminal,
  createUserTerminal,
  readUserTerminal,
  resizeUserTerminal,
  writeUserTerminal,
  type UserTerminalScope,
} from "./userTerminalDesktop";

const scope: UserTerminalScope = {
  projectId: "project-1",
  projectGeneration: "3",
  chatId: "chat-1",
};

const handle = {
  sessionId: "f99cd56a-32d9-4600-a1f6-269e05734268",
  revision: "1",
};

beforeEach(() => {
  invokeNative.mockReset();
});

it("sends only typed scope and dimensions for creation", async () => {
  invokeNative.mockResolvedValue({
    ...handle,
    shellPath: "C:\\Windows\\System32\\cmd.exe",
  });
  await expect(createUserTerminal(scope, 80, 24)).resolves.toMatchObject(
    handle,
  );
  expect(invokeNative).toHaveBeenCalledWith("terminal_create", {
    request: { scope, columns: 80, rows: 24 },
  });
});

it("decodes a bounded binary output page and preserves cursor strings", async () => {
  invokeNative.mockResolvedValue({
    bytesBase64: "AAH/",
    nextCursor: "3",
    truncations: "0",
    truncated: false,
  });
  const page = await readUserTerminal(scope, handle, "0");
  expect(page.bytes).toEqual(new Uint8Array([0, 1, 255]));
  expect(page.nextCursor).toBe("3");
  expect(invokeNative).toHaveBeenCalledWith("terminal_read", {
    request: { scope, handle, cursor: "0" },
  });
});

it("decodes exactly 48,000 output bytes and refuses a larger page", async () => {
  invokeNative.mockResolvedValue({
    bytesBase64: btoa("\0".repeat(48_000)),
    nextCursor: "48000",
    truncated: false,
  });
  await expect(readUserTerminal(scope, handle, "0")).resolves.toEqual({
    bytes: new Uint8Array(48_000),
    nextCursor: "48000",
    truncated: false,
  });
  invokeNative.mockResolvedValue({
    bytesBase64: btoa("\0".repeat(48_001)),
    nextCursor: "48001",
    truncated: false,
  });
  await expect(readUserTerminal(scope, handle, "0")).rejects.toThrow(
    "Invalid terminal output page",
  );
});

it("sends input up to 4,096 UTF-8 bytes and rejects empty or larger input", async () => {
  const received: string[] = [];
  invokeNative.mockImplementation(async (_command, { request }) => {
    received.push(request.text);
  });
  const accepted = ["x".repeat(4_096), "é".repeat(2_048)];
  for (const text of accepted) await writeUserTerminal(scope, handle, text);
  expect(received).toEqual(accepted);
  for (const text of ["", "x".repeat(4_097), "é".repeat(2_048) + "x"])
    await expect(writeUserTerminal(scope, handle, text)).rejects.toThrow();
  expect(received).toEqual(accepted);
});

const acceptedGrids = [
  [1, 1],
  [500, 300],
] as const;
const rejectedGrids = [
  [0, 24],
  [80, 0],
  [501, 24],
  [80, 301],
  [80.5, 24],
  [80, Number.NaN],
] as const;

it("creates terminals only within the native 500 by 300 grid", async () => {
  const grids: number[][] = [];
  const session = { ...handle, shellPath: "/bin/sh" };
  invokeNative.mockImplementation(async (_command, { request }) => {
    grids.push([request.columns, request.rows]);
    return session;
  });
  for (const [columns, rows] of acceptedGrids)
    await expect(createUserTerminal(scope, columns, rows)).resolves.toEqual(
      session,
    );
  expect(grids).toEqual(acceptedGrids);
  for (const [columns, rows] of rejectedGrids)
    await expect(createUserTerminal(scope, columns, rows)).rejects.toThrow(
      "Invalid terminal dimensions",
    );
  expect(grids).toEqual(acceptedGrids);
});

it("resizes terminals only within the native 500 by 300 grid", async () => {
  const grids: number[][] = [];
  invokeNative.mockImplementation(async (_command, { request }) => {
    grids.push([request.columns, request.rows]);
  });
  for (const [columns, rows] of acceptedGrids)
    await resizeUserTerminal(scope, handle, columns, rows);
  expect(grids).toEqual(acceptedGrids);
  for (const [columns, rows] of rejectedGrids)
    await expect(
      resizeUserTerminal(scope, handle, columns, rows),
    ).rejects.toThrow("Invalid terminal dimensions");
  expect(grids).toEqual(acceptedGrids);
});

it("sends only opaque handle fields when a session also carries shell metadata", async () => {
  const session = { ...handle, shellPath: "C:\\Windows\\System32\\cmd.exe" };
  invokeNative.mockResolvedValue({
    bytesBase64: "",
    nextCursor: "0",
    truncations: "0",
    truncated: false,
  });

  await readUserTerminal(scope, session, "0");
  await writeUserTerminal(scope, session, "x");
  await resizeUserTerminal(scope, session, 80, 24);
  await closeUserTerminal(scope, session);

  expect(invokeNative).toHaveBeenNthCalledWith(1, "terminal_read", {
    request: { scope, handle, cursor: "0" },
  });
  expect(invokeNative).toHaveBeenNthCalledWith(2, "terminal_write", {
    request: { scope, handle, text: "x" },
  });
  expect(invokeNative).toHaveBeenNthCalledWith(3, "terminal_resize", {
    request: { scope, handle, columns: 80, rows: 24 },
  });
  expect(invokeNative).toHaveBeenNthCalledWith(4, "terminal_close", {
    request: { scope, handle },
  });
});
