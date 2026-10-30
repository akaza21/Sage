import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FileMarkdownPreview } from "./FileMarkdownPreview";

afterEach(cleanup);

describe("MarkdownBlocks lists", () => {
  it("renders indented items as lists nested in their parent item", () => {
    const { container } = render(
      <FileMarkdownPreview
        source={["- one", "  - one.a", "    1. deep", "- **two**"].join("\n")}
      />,
    );
    const top = container.querySelector("article > ul")!;
    expect(top.children).toHaveLength(2);
    const first = top.children[0];
    expect(first.firstChild?.textContent).toBe("one");
    const nested = first.querySelector(":scope > ul")!;
    expect(nested).toHaveTextContent("one.a");
    expect(nested.querySelector(":scope > li > ol > li")).toHaveTextContent(
      "deep",
    );
    expect(top.children[1].querySelector("strong")).toHaveTextContent("two");
  });

  it("renders every item when the first one has no text", () => {
    const { container } = render(
      <FileMarkdownPreview source={"- <br>\n  - child\n- sibling"} />,
    );
    expect(
      [...container.querySelectorAll("li")].map(
        (item) => item.firstChild?.textContent,
      ),
    ).toEqual(["child", "sibling"]);
  });

  it("renders task items as read-only checkboxes", () => {
    render(<FileMarkdownPreview source={"- [ ] todo\n- [x] done"} />);
    const items = within(screen.getByRole("list")).getAllByRole("listitem");
    expect(items.map((item) => item.className)).toEqual([
      "markdown-task",
      "markdown-task",
    ]);
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes.map((box) => (box as HTMLInputElement).checked)).toEqual([
      false,
      true,
    ]);
    expect(boxes.every((box) => (box as HTMLInputElement).disabled)).toBe(true);
    expect(items[0]).toHaveTextContent("todo");
    expect(boxes[0]).toHaveAccessibleName("todo");
  });

  it("starts a top-level ordered list at its first number", () => {
    const { container } = render(
      <FileMarkdownPreview source={"3. three\n4. four"} />,
    );
    expect(container.querySelector("ol")).toHaveAttribute("start", "3");
  });

  it("shows a web link's address on hover", () => {
    render(<FileMarkdownPreview source="See [docs](https://example.test)." />);
    expect(screen.getByText("docs")).toHaveAttribute(
      "title",
      "https://example.test",
    );
    expect(screen.queryByRole("link")).toBeNull();
  });
});
