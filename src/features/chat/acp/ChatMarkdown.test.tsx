import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ChatMarkdown } from "./ChatMarkdown";
import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  renderMarkdownPreview,
} from "../../files/markdownPreview";
import * as markdownPreview from "../../files/markdownPreview";
import { acpDesktop } from "../../../services/acpDesktop";
import { openAgentArtifact } from "../../../app/openAgentArtifact";
import { useWorkbenchStore } from "../../../app/store";
import type { AgentArtifactOwner } from "../../../domain/agentArtifact";

vi.mock("../../../app/openAgentArtifact", () => ({
  openAgentArtifact: vi.fn().mockResolvedValue(true),
  agentArtifactOwnerIsCurrent: vi.fn().mockReturnValue(true),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("ChatMarkdown code blocks", () => {
  it("opens a natively resolved assistant file link through Chat admission", async () => {
    const resource = { rootId: "root-a", relativePath: "README.md" };
    vi.spyOn(acpDesktop, "resolveChatFileReference").mockResolvedValue({
      resource,
      lineNumber: 12,
    });
    const owner = {
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
    } as AgentArtifactOwner;
    const openFileLocation = vi
      .spyOn(useWorkbenchStore.getState(), "openFileLocation")
      .mockResolvedValue();
    render(
      <ChatMarkdown
        text="See [README.md](README.md:12)"
        fileReferenceOwner={owner}
      />,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Open README.md" }),
    );
    expect(acpDesktop.resolveChatFileReference).toHaveBeenCalledWith(
      "project-a",
      "README.md:12",
    );
    expect(openAgentArtifact).toHaveBeenCalledWith({
      owner,
      target: "file",
      resource,
    });
    await waitFor(() =>
      expect(openFileLocation).toHaveBeenCalledWith({
        projectId: "project-a",
        transitionEpoch: useWorkbenchStore.getState().nativeTransitionEpoch,
        resource,
        lineNumber: 12,
        columnNumber: 1,
        selection: { kind: "caret" },
      }),
    );
  });
  it("keeps a repeated plain label inert before the inline link", async () => {
    vi.spyOn(acpDesktop, "resolveChatFileReference").mockResolvedValue({
      resource: { rootId: "root-a", relativePath: "README.md" },
      lineNumber: null,
    });
    const owner = {
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
    } as AgentArtifactOwner;
    render(
      <ChatMarkdown
        text="README.md then [README.md](README.md)"
        fileReferenceOwner={owner}
      />,
    );

    const link = await screen.findByRole("button", { name: "Open README.md" });
    expect(link.previousSibling?.textContent).toBe("README.md then ");
    expect(link.parentElement?.textContent).toBe("README.md then README.md");
  });
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
