import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentDiffRef } from "../../domain/agentDiff";
import { diffLineCommentContext } from "../../domain/attachedContext";
import { ComposerAttachments } from "./ComposerAttachments";

const ref = {
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
  runtimeEpoch: "1",
  turnId: "turn-a",
  itemId: "item-a",
  diffOrdinal: 0,
  snapshotRevision: "1",
  rootId: "root-a",
  relativePath: "src/app.ts",
  contentDigest: "a".repeat(64),
} as AgentDiffRef;

afterEach(cleanup);

describe("ComposerAttachments", () => {
  it("shows a removable page chip with its source URL available for review", () => {
    const page = {
      kind: "browserPage" as const,
      tabId: "browser-1",
      url: "https://example.com/article",
      title: "Example article",
      text: "Article text",
    };
    const onRemove = vi.fn();
    render(
      <ComposerAttachments
        contexts={[page]}
        disabled={false}
        onRemove={onRemove}
      />,
    );
    expect(screen.getByText("Page · Example article")).toHaveAttribute(
      "title",
      page.url,
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "Remove Page · Example article context",
      }),
    );
    expect(onRemove).toHaveBeenCalledExactlyOnceWith(page);
  });
  it.each([
    { rootId: "root-b" },
    { snapshotRevision: "2" },
    { itemId: "item-b" as AgentDiffRef["itemId"] },
  ])("preserves chip identity across immutable ref changes: %j", (identity) => {
    const first = diffLineCommentContext(ref, "new", 3, 3, "Review this");
    const second = diffLineCommentContext(
      { ...ref, ...identity },
      "new",
      3,
      3,
      "Review this",
    );
    const onRemove = vi.fn();
    const { rerender } = render(
      <ComposerAttachments
        contexts={[first, second]}
        disabled={false}
        onRemove={onRemove}
      />,
    );
    const [firstChip, secondChip] = screen.getAllByRole("listitem");
    expect(firstChip.textContent).toBe(secondChip.textContent);

    rerender(
      <ComposerAttachments
        contexts={[second, first]}
        disabled={false}
        onRemove={onRemove}
      />,
    );
    const reordered = screen.getAllByRole("listitem");
    expect(reordered[0]).toBe(secondChip);
    expect(reordered[1]).toBe(firstChip);
    fireEvent.click(within(reordered[0]).getByRole("button"));
    expect(onRemove).toHaveBeenCalledExactlyOnceWith(second);
  });
});
