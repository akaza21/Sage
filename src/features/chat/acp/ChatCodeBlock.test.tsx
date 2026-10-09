import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ChatCodeBlock } from "./ChatCodeBlock";
import {
  highlightCodeSource,
  type CodeSpan,
} from "../../files/CodeMirrorEditorRuntime";

const highlightMock = vi.hoisted(() => vi.fn());
vi.mock("../../files/CodeMirrorEditorRuntime", () => ({
  highlightCodeSource: highlightMock,
}));

beforeEach(() => {
  vi.mocked(highlightCodeSource).mockReset();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("lazy code cards", () => {
  it("keeps source plain during loading and discards stale completions including A-B-A", async () => {
    const finish: ((spans: CodeSpan[]) => void)[] = [];
    vi.mocked(highlightCodeSource).mockImplementation(
      () => new Promise((resolve) => finish.push(resolve)),
    );
    const { container, rerender } = render(
      <ChatCodeBlock source="first" language="py" />,
    );
    await waitFor(() => expect(finish).toHaveLength(1));
    expect(container.querySelector("code")?.textContent).toBe("first");
    expect(container.querySelector("code span")).toBeNull();
    rerender(<ChatCodeBlock source="second" language="py" />);
    await waitFor(() => expect(finish).toHaveLength(2));
    rerender(<ChatCodeBlock source="first" language="py" />);
    await waitFor(() => expect(finish).toHaveLength(3));
    await act(async () => {
      finish[0]([{ from: 0, to: 5, className: "stale" }]);
      finish[1]([{ from: 0, to: 6, className: "stale" }]);
    });
    expect(container.querySelector(".stale")).toBeNull();
    await act(async () =>
      finish[2]([{ from: 0, to: 5, className: "current" }]),
    );
    expect(container.querySelector(".current")).toHaveTextContent("first");
    expect(container.querySelector(".cm-editor")).toBeNull();
  });

  it("loads only the first visible supported fence and leaves unknown languages plain", async () => {
    let visible!: IntersectionObserverCallback;
    const observe = vi.fn();
    const disconnect = vi.fn();
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(callback: IntersectionObserverCallback) {
          visible = callback;
        }
        observe = observe;
        disconnect = disconnect;
      },
    );
    vi.mocked(highlightCodeSource).mockResolvedValue([]);
    const { rerender } = render(
      <ChatCodeBlock source="print(1)" language="python" />,
    );
    expect(observe).toHaveBeenCalled();
    expect(highlightCodeSource).not.toHaveBeenCalled();
    await act(async () =>
      visible(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      ),
    );
    await waitFor(() =>
      expect(highlightCodeSource).toHaveBeenCalledWith(
        "print(1)",
        "python",
        expect.any(Function),
      ),
    );
    expect(disconnect).toHaveBeenCalled();
    rerender(<ChatCodeBlock source="unknown" language="unknown" />);
    expect(screen.getByText("unknown", { selector: "code" })).toBeVisible();
    expect(highlightCodeSource).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["tsx", "TypeScript JSX", "typescript-jsx"],
    ["jsx", "JavaScript JSX", "javascript-jsx"],
    ["rs", "Rust", "rust"],
    ["ts", "TypeScript", "typescript"],
  ])(
    "preserves the %s label and parser alias",
    async (language, label, parser) => {
      vi.mocked(highlightCodeSource).mockRejectedValue(
        new Error("unavailable"),
      );
      const { container } = render(
        <ChatCodeBlock source="literal" language={language} />,
      );
      expect(
        screen.getByRole("region", { name: `${label} code` }),
      ).toBeVisible();
      await waitFor(() =>
        expect(highlightCodeSource).toHaveBeenCalledWith(
          "literal",
          parser,
          expect.any(Function),
        ),
      );
      expect(container.querySelector("code")?.textContent).toBe("literal");
      expect(container.querySelector("code span")).toBeNull();
    },
  );

  it("does not start obsolete highlighting after the lazy import settles", async () => {
    const visible: IntersectionObserverCallback[] = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(callback: IntersectionObserverCallback) {
          visible.push(callback);
        }
        observe = vi.fn();
        disconnect = vi.fn();
      },
    );
    const intersect = (index: number) =>
      visible[index](
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    vi.mocked(highlightCodeSource).mockResolvedValue([]);
    const { rerender, unmount } = render(
      <ChatCodeBlock source="old" language="python" />,
    );
    act(() => {
      intersect(0);
      rerender(<ChatCodeBlock source="current" language="python" />);
    });
    await act(() => vi.dynamicImportSettled());
    expect(highlightCodeSource).not.toHaveBeenCalled();
    act(() => intersect(1));
    await waitFor(() => expect(highlightCodeSource).toHaveBeenCalledTimes(1));
    expect(vi.mocked(highlightCodeSource).mock.calls[0].slice(0, 2)).toEqual([
      "current",
      "python",
    ]);
    const current = vi.mocked(highlightCodeSource).mock.calls[0][2]!;
    expect(current()).toBe(true);
    unmount();
    expect(current()).toBe(false);
  });

  it("does not show stale copy success after source changes", async () => {
    let finish!: () => void;
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn(
          () =>
            new Promise<void>((resolve) => {
              finish = resolve;
            }),
        ),
      },
    });
    const { rerender } = render(<ChatCodeBlock source="old" language="" />);
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    rerender(<ChatCodeBlock source="new" language="" />);
    await act(async () => finish());
    expect(screen.getByRole("status")).toHaveTextContent("");
  });
});
