import { beforeEach, expect, it, vi } from "vitest";

vi.mock("../ports/nativeInvoke", () => ({ invokeNative: vi.fn() }));

import { invokeNative } from "../ports/nativeInvoke";
import {
  discardPendingChatImage,
  getChatImageThumbnail,
} from "./imageArtifactDesktop";

const invoke = vi.mocked(invokeNative);

beforeEach(() => {
  invoke.mockReset();
});

it("admits a bounded PNG thumbnail and an evicted placeholder", async () => {
  const request = {
    projectId: "project",
    projectGeneration: "1",
    chatId: "chat",
    artifactId: "f4a4e31e-9d0b-4bc4-a709-a2f625788510",
  };
  invoke.mockResolvedValueOnce({ dataUrl: "data:image/png;base64,YQ==" });
  await expect(getChatImageThumbnail(request)).resolves.toBe(
    "data:image/png;base64,YQ==",
  );
  expect(invoke).toHaveBeenCalledWith("get_chat_image_thumbnail", { request });
  invoke.mockResolvedValueOnce(null);
  await expect(getChatImageThumbnail(request)).resolves.toBeNull();
});

it("rejects an unbounded or unsafe native preview response", async () => {
  const request = {
    projectId: "project",
    projectGeneration: "1",
    chatId: "chat",
    artifactId: "f4a4e31e-9d0b-4bc4-a709-a2f625788510",
  };
  for (const dataUrl of [
    "data:image/svg+xml;base64,YQ==",
    "data:image/png;base64," + "A".repeat(350_000),
    "data:image/png;base64,not-base64!",
  ]) {
    invoke.mockResolvedValueOnce({ dataUrl });
    await expect(getChatImageThumbnail(request)).rejects.toThrow(
      "Image thumbnail response is invalid",
    );
  }
});

it("requires a typed discard acknowledgment", async () => {
  const request = {
    projectId: "project",
    projectGeneration: "1",
    chatId: "chat",
    artifactId: "f4a4e31e-9d0b-4bc4-a709-a2f625788510",
  };
  invoke.mockResolvedValueOnce(true);
  await expect(discardPendingChatImage(request)).resolves.toBe(true);
  invoke.mockResolvedValueOnce("true");
  await expect(discardPendingChatImage(request)).rejects.toThrow(
    "Image discard response is invalid",
  );
});
