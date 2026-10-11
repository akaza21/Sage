import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ChatMarkdown } from "./ChatMarkdown";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("Chat syntax highlighting through the real lazy runtime", () => {
  it.each([
    ["ts", "export const example = 2;", "export"],
    ["typescript", "export const example = 2;", "export"],
    ["python", "def valid(value):\n  return True", "def"],
    ["javascript", "const value = 2;", "const"],
    ["rust", "fn main() {}", "fn"],
  ])(
    "renders keyword tokens with shared palette rules for an admitted %s fence",
    async (language, source, keyword) => {
      const { container } = render(
        <ChatMarkdown text={`\`\`\`${language}\n${source}\n\`\`\``} />,
      );
      expect(container.querySelector("code span")).toBeNull();
      await waitFor(() => {
        const tokens = [...container.querySelectorAll("code span")];
        expect(tokens.some((token) => token.textContent === keyword)).toBe(
          true,
        );
      });
      const keywordToken = [...container.querySelectorAll("code span")].find(
        (token) => token.textContent === keyword,
      )!;
      const rules = [...document.styleSheets].flatMap((sheet) => [
        ...sheet.cssRules,
      ]);
      expect(
        rules.some(
          (rule) =>
            rule.cssText.includes(`.${keywordToken.className}`) &&
            rule.cssText.includes("var(--doc-syntax-keyword)"),
        ),
      ).toBe(true);
      expect(container.querySelector("code")?.textContent).toBe(source);
    },
  );

  it("highlights a visible streaming fence after settlement and remount", async () => {
    const callbacks: IntersectionObserverCallback[] = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(callback: IntersectionObserverCallback) {
          callbacks.push(callback);
        }
        observe() {}
        disconnect() {}
      },
    );
    const { container, rerender, unmount } = render(
      <ChatMarkdown text={"```ts\nexport const"} streaming />,
    );
    const intersect = () =>
      callbacks.at(-1)!(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    await act(async () => intersect());
    await waitFor(() =>
      expect(container.querySelector("code span")).not.toBeNull(),
    );
    const text = "```ts\nexport const example = 2;\n```";
    rerender(<ChatMarkdown text={text} />);
    await act(async () => intersect());
    await waitFor(() =>
      expect(
        [...container.querySelectorAll("code span")].some(
          (token) => token.textContent === "2",
        ),
      ).toBe(true),
    );
    unmount();
    const reopened = render(<ChatMarkdown text={text} />);
    await act(async () => intersect());
    await waitFor(() =>
      expect(reopened.container.querySelector("code span")).not.toBeNull(),
    );
  });
});
