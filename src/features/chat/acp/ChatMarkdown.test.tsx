import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ChatMarkdown } from "./ChatMarkdown";
import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  renderMarkdownPreview,
} from "../../files/markdownPreview";
import * as markdownPreview from "../../files/markdownPreview";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("ChatMarkdown code blocks", () => {
  it("labels the language and copies the exact retained source", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(<ChatMarkdown text={"```ts\nconst answer = 42;\n```"} />);

    expect(
      screen.getByRole("region", { name: "TypeScript code" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));

    expect(writeText).toHaveBeenCalledWith("const answer = 42;");
    expect(await screen.findByText("Copied")).toBeInTheDocument();
  });

  it("keeps an unfinished streaming Mermaid fence as receiving source", () => {
    const { rerender } = render(
      <ChatMarkdown text={"```mermaid\ngraph TD\nA -->"} streaming />,
    );

    expect(
      screen.getByRole("region", { name: "Mermaid code" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("status", { name: "Receiving diagram source" }),
    ).toBeVisible();
    expect(screen.queryByRole("img", { name: /Mermaid/ })).toBeNull();
    expect(screen.queryByText(/could not be drawn/i)).toBeNull();

    rerender(
      <ChatMarkdown text={"```mermaid\ngraph TD\nA --> B\n```"} streaming />,
    );
    expect(
      screen.queryByRole("status", { name: "Receiving diagram source" }),
    ).toBeNull();
  });

  it("does not mark a closed retained Mermaid block as receiving when later source is capped", () => {
    const text = [
      "```mermaid",
      "graph TD",
      "A --> B",
      "```",
      ...Array.from(
        { length: MAX_MARKDOWN_PREVIEW_BLOCKS },
        (_, index) => `\n# Block ${index}`,
      ),
      "\n```mermaid\ngraph TD\nC -->",
    ].join("\n");
    expect(renderMarkdownPreview(text).truncated).toBe(true);
    render(<ChatMarkdown text={text} streaming />);
    expect(
      screen.queryByRole("status", { name: "Receiving diagram source" }),
    ).toBeNull();
  });

  it("parses only when exact markdown text changes", () => {
    const parse = vi.spyOn(markdownPreview, "renderMarkdownPreview");
    const { rerender } = render(<ChatMarkdown text="Retained text" />);
    expect(parse).toHaveBeenCalledTimes(1);
    rerender(<ChatMarkdown text="Retained text" />);
    expect(parse).toHaveBeenCalledTimes(1);
    rerender(<ChatMarkdown text="Revised text" />);
    expect(parse).toHaveBeenCalledTimes(2);
  });
});
