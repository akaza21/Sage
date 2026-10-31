import { expect, it } from "vitest";
import { page } from "../features/chat/acp/acpProjection.fixture";
import { mergeProjectionPage } from "../app/acpProjectionMerge";
import {
  withPresentedAttention,
  presentedAttentionPosition,
} from "./chatAttention";

it("attention retains the latest detail observation when merging earlier history", () => {
  const current = withPresentedAttention(page("completed"), "1");
  const earlier = page("completed");
  expect(presentedAttentionPosition(earlier)).toBeUndefined();
  expect(
    presentedAttentionPosition(mergeProjectionPage(current, earlier)),
  ).toBe("1");
});
