import { afterEach, expect, it, vi } from "vitest";
import { attention } from "../domain/chatAttention.fixture";
import { page } from "../features/chat/acp/acpProjection.fixture";
import { mergeProjectionPage } from "../app/acpProjectionMerge";
import {
  chatAttention,
  observeAttentionBeforeDetail,
  presentedAttentionPosition,
} from "./chatAttention";

afterEach(() => vi.restoreAllMocks());

it("attention retries failed observation with a newly paired detail page", async () => {
  vi.spyOn(chatAttention, "get")
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValueOnce([attention({ activityPosition: "3" })]);
  const load = vi.fn(async () => page("completed"));
  await expect(
    observeAttentionBeforeDetail("project-1", "chat-1", load),
  ).rejects.toThrow("offline");
  expect(load).not.toHaveBeenCalled();
  const detail = await observeAttentionBeforeDetail(
    "project-1",
    "chat-1",
    load,
  );
  expect(load).toHaveBeenCalledTimes(1);
  expect(presentedAttentionPosition(detail)).toBe("3");
});

it("attention retains the latest detail observation when merging earlier history", async () => {
  vi.spyOn(chatAttention, "get").mockResolvedValue([attention()]);
  const current = await observeAttentionBeforeDetail(
    "project-1",
    "chat-1",
    async () => page("completed"),
  );
  const earlier = page("completed");
  expect(presentedAttentionPosition(earlier)).toBeUndefined();
  expect(
    presentedAttentionPosition(mergeProjectionPage(current, earlier)),
  ).toBe("1");
});
