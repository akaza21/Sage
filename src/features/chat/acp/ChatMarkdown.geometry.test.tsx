import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { ChatMarkdown } from "./ChatMarkdown";
import { FileMarkdownPreview } from "../../files/FileMarkdownPreview";

afterEach(cleanup);

describe("CP-28 Chat list gutter", () => {
  it.each([
    [1, 1, "1ch"],
    [9, 1, "1ch"],
    [9, 2, "2ch"],
    [10, 2, "2ch"],
    [100, 2, "3ch"],
    [999999999, 2, "10ch"],
  ])(
    "reserves the widest native ordinal for start %i and %i items without changing Editor lists",
    (start, count, width) => {
      const source = [
        "- Bullet with wrapping prose",
        "",
        ...Array.from(
          { length: count },
          (_, index) =>
            `${Math.min(999999999, Number(start) + index)}. Ordered item with wrapping prose`,
        ),
      ].join("\n");
      const { container, unmount } = render(<ChatMarkdown text={source} />);
      expect(
        container
          .querySelector<HTMLElement>(".acp-markdown")!
          .style.getPropertyValue("--chat-list-marker-width"),
      ).toBe(width);
      const lists = screen.getAllByRole("list");
      expect(lists).toHaveLength(2);
      expect(lists[1]).toHaveAttribute("start", String(start));
      expect(lists[1].querySelectorAll("li")).toHaveLength(Number(count));
      unmount();
      const editor = render(<FileMarkdownPreview source={source} />);
      expect(screen.getAllByRole("list")[1]).toHaveAttribute(
        "start",
        String(start),
      );
      expect(
        editor.container.querySelector('[style*="--chat-list-marker-width"]'),
      ).toBeNull();
    },
  );
});
