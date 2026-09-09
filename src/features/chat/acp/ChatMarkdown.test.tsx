import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ChatMarkdown } from "./ChatMarkdown";
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
