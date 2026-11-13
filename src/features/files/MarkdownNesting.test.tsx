import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ChatMarkdown } from "../chat/acp/ChatMarkdown";
import { FileMarkdownPreview } from "./FileMarkdownPreview";
import { MarkdownBlocks } from "./MarkdownBlocks";
import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  renderMarkdownPreview,
} from "./markdownPreview";

afterEach(cleanup);

it.each(
  ["preview", "chat"].flatMap((surface) =>
    [
      ["code", "~~~text\n  example\n  ~~~", "DIV"],
      ["table", "| H |\n  | - |\n  | example |", "DIV"],
      ["rule", "---", "HR"],
    ].map(([kind, source, tag]) => ({ surface, kind, source, tag })),
  ),
)(
  "keeps $kind between item paragraphs before its sibling in $surface",
  ({ surface, source, tag }) => {
    const text = `- before\n\n  ${source}\n\n  after\n- second`;
    const { container } = render(
      surface === "chat" ? (
        <ChatMarkdown text={text} />
      ) : (
        <FileMarkdownPreview source={text} />
      ),
    );
    const items = container.querySelectorAll("li");
    expect(items).toHaveLength(2);
    expect([...items[0].children].map((child) => child.tagName)).toEqual([
      "P",
      tag,
      "P",
    ]);
    expect(items[0].firstElementChild).toHaveTextContent("before");
    expect(items[0].lastElementChild).toHaveTextContent("after");
    expect(items[0].nextElementSibling).toBe(items[1]);
    expect(items[1]).toHaveTextContent("second");
    if (tag !== "HR") expect(items[0].children[1]).toHaveTextContent("example");
  },
);

it.each(["quote", "list"])(
  "shares the document block cap with empty fenced components inside a %s",
  (kind) => {
    const fence =
      kind === "quote" ? "> ```python\n> ```\n>\n" : "- ```python\n  ```\n\n";
    const source = "# Before\n\n" + fence.repeat(100) + "\n\n# Skipped";
    const document = renderMarkdownPreview(source);
    const { container } = render(<MarkdownBlocks document={document} />);
    expect(container.querySelectorAll("pre")).toHaveLength(
      MAX_MARKDOWN_PREVIEW_BLOCKS - 2,
    );
    expect(screen.getByRole("heading", { name: "Before" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Skipped" })).toBeNull();
    expect(document.truncated).toBe(true);
    const exact = renderMarkdownPreview(
      "# Before\n\n" + fence.repeat(MAX_MARKDOWN_PREVIEW_BLOCKS - 2),
    );
    expect(exact.truncated).toBe(false);
  },
);

it("keeps formatting and file links on their nested paragraph and table cell", () => {
  const document = renderMarkdownPreview(
    "- **Before**\n\n  > *Quoted* [read](a.ts)\n  >\n  > | H |\n  > | - |\n  > | `cell` [next](b.ts) |\n\n  **After**",
  );
  const { container } = render(
    <MarkdownBlocks
      document={document}
      renderLink={(index) => (
        <button>{document.fileLinks[index].destination}</button>
      )}
    />,
  );
  expect(
    [...container.querySelectorAll("strong")].map(
      (element) => element.textContent,
    ),
  ).toEqual(["Before", "After"]);
  expect(container.querySelector("blockquote em")).toHaveTextContent("Quoted");
  expect(container.querySelector("td code")).toHaveTextContent("cell");
  expect(
    screen.getByRole("button", { name: "a.ts" }).closest("blockquote"),
  ).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "b.ts" }).closest("td"),
  ).toBeTruthy();
});

it.each(["preview", "chat"])(
  "keeps numbered commands inside their step and before the next step in %s",
  (surface) => {
    const source =
      "1. Run this command\n\n   ```sh\n   npm install\n   ```\n\n2. Start the app";
    const { container } = render(
      surface === "chat" ? (
        <ChatMarkdown text={source} />
      ) : (
        <FileMarkdownPreview source={source} />
      ),
    );
    const steps = screen.getAllByRole("listitem");
    expect(steps).toHaveLength(2);
    const command = steps[0].querySelector("pre")!;
    expect(command).toHaveTextContent("npm install");
    expect(
      command.compareDocumentPosition(steps[1]) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      [...container.querySelectorAll("li, pre")].map((node) => node.tagName),
    ).toEqual(["LI", "PRE", "LI"]);
    if (surface === "chat")
      expect(
        within(steps[0]).getByRole("button", { name: "Copy code" }),
      ).toBeVisible();
  },
);

it.each(["preview", "chat"])(
  "keeps a paragraph after its nested list in %s",
  (surface) => {
    const source = "- Before\n  - Child\n\n  After\n- Sibling";
    const { container } = render(
      surface === "chat" ? (
        <ChatMarkdown text={source} />
      ) : (
        <FileMarkdownPreview source={source} />
      ),
    );
    const parent = container.querySelector("li")!;
    expect(parent.textContent).toBe("BeforeChildAfter");
    expect([...parent.children].map((node) => node.tagName)).toEqual([
      "P",
      "UL",
      "P",
    ]);
    expect(parent.lastElementChild).toHaveTextContent("After");
  },
);

it("renders blockquote children in order with the same code actions", () => {
  render(
    <ChatMarkdown
      text={
        "> Before\n>\n> ```sh\n> npm install\n> ```\n>\n> - Child\n>\n> After"
      }
    />,
  );
  const quote = screen.getByText("Before").closest("blockquote")!;
  expect(quote).toHaveTextContent(/Before.*npm install.*Child.*After/s);
  expect(
    within(quote).getByRole("button", { name: "Copy code" }),
  ).toBeVisible();
  expect([...quote.children].map((node) => node.tagName)).toEqual([
    "P",
    "DIV",
    "UL",
    "P",
  ]);
});

it.each(["preview", "chat"])(
  "preserves quoted Python newlines and indentation in %s DOM text",
  (surface) => {
    const source =
      "> ```python\n> def greet(name):\n>     if name:\n>         print(name)\n>\n>     return name\n> ```";
    const { container } = render(
      surface === "chat" ? (
        <ChatMarkdown text={source} />
      ) : (
        <FileMarkdownPreview source={source} />
      ),
    );
    expect(container.querySelector("blockquote pre")?.textContent).toBe(
      "def greet(name):\n    if name:\n        print(name)\n\n    return name",
    );
    if (surface === "chat")
      expect(screen.getByRole("button", { name: "Copy code" })).toBeVisible();
  },
);

it.each(["preview", "chat"])(
  "renders quoted headings, tables, Mermaid and nested containers in order in %s",
  async (surface) => {
    const source =
      "> ## Quoted heading\n>\n> | Name | Value |\n> | --- | --- |\n> | key | **value** |\n>\n> ```mermaid\n> flowchart LR\n> A --> B\n> ```\n>\n> - Child\n>   > Nested quote\n>\n> After";
    const { container } = render(
      surface === "chat" ? (
        <ChatMarkdown text={source} />
      ) : (
        <FileMarkdownPreview source={source} />
      ),
    );
    const quote = container.querySelector("blockquote")!;
    expect(
      within(quote).getByRole("heading", { level: 2, name: "Quoted heading" }),
    ).toBeVisible();
    const table = within(quote).getByRole("table");
    expect(within(table).getByRole("cell", { name: "value" })).toContainHTML(
      "<strong>value</strong>",
    );
    expect(
      await within(quote).findByRole("img", { name: "Mermaid diagram" }),
    ).toBeVisible();
    expect(quote.querySelector("li blockquote")).toHaveTextContent(
      "Nested quote",
    );
    expect([...quote.children].map((node) => node.tagName)).toEqual([
      "H2",
      "DIV",
      "DIV",
      "UL",
      "P",
    ]);
    expect(quote.lastElementChild).toHaveTextContent("After");
  },
);
