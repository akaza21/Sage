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

beforeEach(() => invokeNative.mockReset());

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

it("rejects oversized input before invoking native", async () => {
  await expect(
    writeUserTerminal(scope, handle, "x".repeat(4_097)),
  ).rejects.toThrow();
  expect(invokeNative).not.toHaveBeenCalled();
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
