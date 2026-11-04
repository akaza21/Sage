import { MarkdownBlocks } from "./MarkdownBlocks";
import { renderMarkdownPreview } from "./markdownPreview";
import { ChatMarkdown } from "../chat/acp/ChatMarkdown";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FileMarkdownPreview } from "./FileMarkdownPreview";

afterEach(cleanup);

describe("MarkdownBlocks lists", () => {
  it.each(["preview", "chat"])(
    "keeps a two-space item beside a tab-separated parent in %s",
    (surface) => {
      const source = "-\tparent\n  - sibling";
      const { container } = render(
        surface === "preview" ? (
          <FileMarkdownPreview source={source} />
        ) : (
          <ChatMarkdown text={source} />
        ),
      );
      expect(container.querySelector("ul")!.outerHTML).toBe(
        "<ul><li>parent</li><li>sibling</li></ul>",
      );
    },
  );

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

it("renders the parsed rule, headings, literal code and quote as DOM blocks", () => {
  const document = renderMarkdownPreview(
    "Title\n===\n\n---\n\nSubtitle\n---\n\n~~~js\n*a*\n~~~\n\n> one\n>\n> two",
  );
  const { container } = render(<MarkdownBlocks document={document} />);
  expect(container.innerHTML).toBe(
    "<h1>Title</h1><hr><h2>Subtitle</h2><div><pre>*a*</pre></div><blockquote>one two</blockquote>",
  );
});

it.each(["preview", "chat"])("renders rules in the %s surface", (surface) => {
  const source = "Intro\n\n* * *\n\nAfter";
  const { container } = render(
    surface === "preview" ? (
      <FileMarkdownPreview source={source} />
    ) : (
      <ChatMarkdown text={source} />
    ),
  );
  expect(screen.getByRole("separator").tagName).toBe("HR");
  expect(container.querySelectorAll("li")).toHaveLength(0);
  expect(container.textContent).toBe("IntroAfter");
});

it.each(["-", "+", "*", "1.", "2)"])(
  "renders an empty %s list and a separate rule",
  (marker) => {
    const { container } = render(
      <MarkdownBlocks document={renderMarkdownPreview(`${marker}\n---`)} />,
    );
    const ordered = /\d/u.test(marker);
    expect(container.innerHTML).toBe(
      ordered
        ? `<ol start="${marker[0]}"><li></li></ol><hr>`
        : "<ul><li></li></ul><hr>",
    );
  },
);

it("renders an over-indented closer as literal code through the end of the preview", () => {
  const { container } = render(
    <MarkdownBlocks
      document={renderMarkdownPreview("~~~js\ncode\n    ~~~\nAfter")}
    />,
  );
  expect(container.innerHTML).toBe(
    "<div><pre>code\n    ~~~\nAfter</pre></div>",
  );
});

it.each([
  ["#\n---", "<h1></h1><hr>"],
  ["   # Title\n---", "<h1>Title</h1><hr>"],
])("renders ATX before a rule in the preview: %s", (source, expected) => {
  const { container, rerender } = render(
    <MarkdownBlocks document={renderMarkdownPreview(source)} />,
  );
  expect(container.innerHTML).toBe(expected);
  rerender(<FileMarkdownPreview source={source} />);
  expect(container.querySelector("article")!.innerHTML).toBe(expected);
});

it.each(["-", "+", "*", "1.", "2)"])(
  "renders a tab-separated %s item between a paragraph and rule",
  (marker) => {
    const source = `Intro\n${marker}\titem\n---`;
    const list = /^\d/u.test(marker)
      ? `<ol start="${marker[0]}"><li>item</li></ol>`
      : "<ul><li>item</li></ul>";
    const expected = `<p>Intro</p>${list}<hr>`;
    const { container, rerender } = render(
      <MarkdownBlocks document={renderMarkdownPreview(source)} />,
    );
    expect(container.innerHTML).toBe(expected);
    rerender(<FileMarkdownPreview source={source} />);
    expect(container.querySelector("article")!.innerHTML).toBe(expected);
  },
);
