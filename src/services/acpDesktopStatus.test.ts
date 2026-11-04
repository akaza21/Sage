import { expect, it, vi } from "vitest";
import type { GetAcpChatPageRequest } from "../domain/acp";
import { invokeNative } from "../ports/nativeInvoke";
import { getChatStatus, safeChatStatus } from "./acpDesktopProjection";

vi.mock("../ports/nativeInvoke", () => ({ invokeNative: vi.fn() }));
const response = {
  projectId: "p",
  chatId: "c",
  title: "Chat",
  latestTurn: {
    state: "waitingApproval",
    updatedAt: 3,
    createdAt: 2,
    terminalAt: null,
  },
};

it("decodes bounded status and rejects malformed native fields", () => {
  expect(safeChatStatus(response)).toEqual(response);
  expect(
    safeChatStatus({ ...response, latestTurn: null })?.latestTurn,
  ).toBeNull();
  for (const invalid of [
    null,
    { ...response, title: "x".repeat(8193) },
    { ...response, latestTurn: {} },
    { ...response, latestTurn: { state: "invented", updatedAt: 3 } },
    { ...response, latestTurn: { state: "running", updatedAt: Infinity } },
  ])
    expect(safeChatStatus(invalid)).toBeNull();
});

it("uses the narrow native command and rejects a foreign owner", async () => {
  const request = {
    projectId: "p",
    projectGeneration: "1",
    chatId: "c",
  } as Omit<GetAcpChatPageRequest, "cursor">;
  vi.mocked(invokeNative).mockResolvedValueOnce(response);
  await expect(getChatStatus(request)).resolves.toEqual(response);
  expect(invokeNative).toHaveBeenCalledWith("get_acp_chat_status", { request });
  vi.mocked(invokeNative).mockResolvedValueOnce({
    ...response,
    chatId: "foreign",
  });
  await expect(getChatStatus(request)).rejects.toThrow(
    "Invalid bounded ACP chat status",
  );
});
