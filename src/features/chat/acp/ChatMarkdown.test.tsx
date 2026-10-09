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
import {
  agentArtifactOwnerIsCurrent,
  openAgentArtifact,
} from "../../../app/openAgentArtifact";
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
  it("copies a longer HTML fence without executing its shorter backtick line", async () => {
    const source = "```\n<script>run()</script>\n<style>body {}</style>";
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const { container } = render(
      <ChatMarkdown text={`\`\`\`\`html\n${source}\n\`\`\`\``} />,
    );
    expect(container.querySelector("code")?.textContent).toBe(source);
    expect(container.querySelector("script, style")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    expect(writeText).toHaveBeenCalledWith(source);
    expect(await screen.findByText("Copied")).toBeVisible();
  });

  it.each(["denied", "stale"])(
    "does not jump to a line after %s file admission",
    async (outcome) => {
      vi.spyOn(acpDesktop, "resolveChatFileReference").mockResolvedValue({
        resource: { rootId: "root-a", relativePath: "README.md" },
        lineNumber: 12,
      });
      if (outcome === "denied")
        vi.mocked(openAgentArtifact).mockResolvedValueOnce(false);
      else vi.mocked(agentArtifactOwnerIsCurrent).mockReturnValueOnce(false);
      const jump = vi
        .spyOn(useWorkbenchStore.getState(), "openFileLocation")
        .mockResolvedValue();
      render(
        <ChatMarkdown
          text="**[readme](README.md:12)**"
          fileReferenceOwner={
            {
              projectId: "project-a",
              chatId: "chat-a",
              sessionId: "session-a",
            } as AgentArtifactOwner
          }
        />,
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "Open README.md" }),
      );
      await Promise.resolve();
      expect(jump).not.toHaveBeenCalled();
    },
  );
  it("preserves adjacent literal inline code in Chat and Editor", async () => {
    const { FileMarkdownPreview } =
      await import("../../files/FileMarkdownPreview");
    const text = "Use `<div>` and `a  b`.";
    for (const component of [
      <ChatMarkdown text={text} />,
      <FileMarkdownPreview source={text} />,
    ]) {
      const { container, unmount } = render(component);
      expect(
        [...container.querySelectorAll("code")].map(
          (element) => element.textContent,
        ),
      ).toEqual(["<div>", "a  b"]);
      unmount();
    }
  });
  it("nests emphasis inside strong and keeps inline code literal", () => {
    const { container } = render(
      <ChatMarkdown text="**strong and *nested*** with `**literal**`" />,
    );
    expect(container.querySelector("strong em")).toHaveTextContent("nested");
    expect(container.querySelector("code")).toHaveTextContent("**literal**");
  });
  it("renders shared semantic formatting and table cells in Chat and Editor preview", async () => {
    const { FileMarkdownPreview } =
      await import("../../files/FileMarkdownPreview");
    const text =
      "# Heading\n\n**strong** and *emphasis* with `inline`\n\n2. ordered\n\n| Name | Value |\n| :--- | ---: |\n| **SQLite** | a\\|b |";
    for (const component of [
      <ChatMarkdown text={text} />,
      <FileMarkdownPreview source={text} />,
    ]) {
      const { container, unmount } = render(component);
      expect(screen.getByRole("heading", { name: "Heading" })).toBeVisible();
      expect(container.querySelector("strong")).toHaveTextContent("strong");
      expect(container.querySelector("em")).toHaveTextContent("emphasis");
      expect(container.querySelector("code")).toHaveTextContent("inline");
      expect(container.querySelector("ol")).toHaveAttribute("start", "2");
      expect(screen.getByRole("columnheader", { name: "Name" })).toHaveStyle({
        textAlign: "left",
      });
      expect(screen.getByRole("cell", { name: "a|b" })).toHaveStyle({
        textAlign: "right",
      });
      unmount();
    }
  });

  it("keeps unlabelled cards unlabelled and wrap independent per fence", () => {
    render(
      <ChatMarkdown text={"```\nfirst\n```\n\n```unknown\nsecond\n```"} />,
    );
    expect(screen.queryByText("Code")).toBeNull();
    const controls = screen.getAllByRole("button", { name: "Wrap code" });
    expect(controls[0]).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(controls[0]);
    expect(controls[0]).toHaveAttribute("aria-pressed", "true");
    expect(controls[1]).toHaveAttribute("aria-pressed", "false");
  });

  it("copies fenced HTML literally and reports clipboard rejection truthfully", async () => {
    const source = "<script>run()</script>\n  <style>body {}</style>  ";
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const { container } = render(
      <ChatMarkdown text={`\`\`\`html\n${source}\n\`\`\``} />,
    );
    expect(container.querySelector("script, style")).toBeNull();
    expect(container.querySelector("pre")).toHaveTextContent(
      "<script>run()</script>",
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    expect(writeText).toHaveBeenCalledWith(source);
    expect(await screen.findByRole("status")).toHaveTextContent("Copy failed");
    expect(screen.queryByText("Copied")).toBeNull();
  });

  it("preserves resolved links inside strong text and table cells", async () => {
    vi.spyOn(acpDesktop, "resolveChatFileReference").mockResolvedValue({
      resource: { rootId: "root-a", relativePath: "README.md" },
      lineNumber: 7,
    });
    const owner = {
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
    } as AgentArtifactOwner;
    render(
      <ChatMarkdown
        text={
          "**[readme](README.md:7)**\n\n| File |\n| --- |\n| *[readme](README.md:7)* |"
        }
        fileReferenceOwner={owner}
      />,
    );
    const links = await screen.findAllByRole("button", {
      name: "Open README.md",
    });
    expect(links).toHaveLength(2);
    expect(links[0].closest("strong")).not.toBeNull();
    expect(links[1].closest("td")?.querySelector("em")).not.toBeNull();
  });
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
