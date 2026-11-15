import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import { setQuickOpenInvoke } from "../quickOpen/quickOpenGateway";
import { ComposerMentionController } from "./ComposerMentionController";

const initial = useWorkbenchStore.getState();
const invoke = vi.fn();

function match(path: string) {
  const [rootId, relativePath] = path.includes(":")
    ? path.split(":")
    : ["root-a", path];
  return { rootId, relativePath, positions: [] };
}

function answer(paths: string[]) {
  return {
    state: "ready",
    results: paths.map(match),
    indexedFiles: paths.length,
    truncated: false,
    refreshing: false,
  };
}

function Composer({ start = "", role }: { start?: string; role?: "combobox" }) {
  const [text, setText] = useState(start);
  const fieldRef = useRef<HTMLTextAreaElement>(null);
  return (
    <form onSubmit={(event) => event.preventDefault()}>
      <div>
        <textarea
          ref={fieldRef}
          aria-label="Message agent"
          role={role}
          aria-expanded={role ? false : undefined}
          value={text}
          onChange={(event) => setText(event.target.value)}
        />
      </div>
      <ComposerMentionController
        fieldRef={fieldRef}
        disabled={false}
        fieldId="message"
        text={text}
      />
    </form>
  );
}

async function type(field: HTMLTextAreaElement, value: string) {
  await act(async () => {
    fireEvent.change(field, { target: { value } });
    field.setSelectionRange(value.length, value.length);
    fireEvent.keyUp(field);
  });
}

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "Project" } as never,
    nativeTransitionEpoch: 4,
  });
  setQuickOpenInvoke(invoke);
});
afterEach(() => {
  cleanup();
  invoke.mockReset();
  setQuickOpenInvoke(null);
  useWorkbenchStore.setState(initial, true);
});

describe("composer @ file picker", () => {
  it("lists project files from the path index and writes the picked token", async () => {
    invoke.mockResolvedValue(answer(["src/main.rs", "docs/my notes.md"]));
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    await type(field, "Read @ma");
    expect(invoke).toHaveBeenLastCalledWith(
      "quick_open_query",
      expect.objectContaining({
        request: expect.objectContaining({
          projectId: "project-a",
          expectedTransitionEpoch: 4,
          query: "ma",
          browse: true,
          limit: 20,
        }),
      }),
    );
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.getAttribute("aria-label"))).toEqual([
      "src/main.rs",
      "docs/my notes.md",
    ]);
    expect(field).toHaveAttribute("aria-controls", "message-mentions");
    expect(field).toHaveAttribute("aria-activedescendant", options[0]!.id);
    expect(field).not.toHaveAttribute("aria-expanded");
    await act(async () => {
      fireEvent.keyDown(field, { key: "ArrowDown" });
    });
    await act(async () => {
      fireEvent.keyDown(field, { key: "Enter" });
    });
    expect(field.value).toBe('Read @"docs/my notes.md" ');
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field).not.toHaveAttribute("aria-controls");
  });

  it("holds Enter while the first list loads and closes on Escape", async () => {
    let resolve: (value: unknown) => void = () => {};
    invoke.mockReturnValue(new Promise((next) => (resolve = next)));
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    await type(field, "@sr");
    const enter = new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
    });
    field.dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(true);
    await act(async () => resolve(answer(["src/lib.rs"])));
    expect(await screen.findByRole("option")).toHaveAccessibleName(
      "src/lib.rs",
    );
    await act(async () => {
      fireEvent.keyDown(field, { key: "Escape" });
    });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field.value).toBe("@sr");
  });

  it("names another root's file by that root and skips paths no token can hold", async () => {
    useWorkbenchStore.setState({
      project: {
        id: "project-a",
        name: "Project",
        roots: [
          { id: "root-a", name: "app" },
          { id: "root-b", name: "lib" },
        ],
      } as never,
    });
    invoke.mockResolvedValue(
      answer(["src/a.rs", "root-b:src/b.rs", 'say "hi".md', 'odd"name.md']),
    );
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    await type(field, "(@sr");
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.getAttribute("aria-label"))).toEqual([
      "src/a.rs",
      "lib/src/b.rs",
      'odd"name.md',
    ]);
    await act(async () => {
      fireEvent.click(options[1]!);
    });
    expect(field.value).toBe("(@lib:src/b.rs ");
  });

  it("stays open while a quoted path with spaces is typed", async () => {
    invoke.mockResolvedValue(answer(["docs/my notes.md"]));
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    await type(field, 'Read @"docs/my no');
    expect(await screen.findByRole("option")).toHaveAccessibleName(
      "docs/my notes.md",
    );
    expect(invoke).toHaveBeenLastCalledWith(
      "quick_open_query",
      expect.objectContaining({
        request: expect.objectContaining({ query: "docs/my no" }),
      }),
    );
    await act(async () => {
      fireEvent.keyDown(field, { key: "Enter" });
    });
    expect(field.value).toBe('Read @"docs/my notes.md" ');
  });

  it("gives a combobox field back its own attributes once the caret leaves the token", async () => {
    invoke.mockResolvedValue(answer(["src/main.rs"]));
    render(<Composer role="combobox" />);
    const field = screen.getByRole<HTMLTextAreaElement>("combobox");
    field.focus();
    await type(field, "@ma");
    await screen.findByRole("option");
    expect(field).toHaveAttribute("aria-expanded", "true");
    expect(field).toHaveAttribute("aria-autocomplete", "list");
    await type(field, "@ma then");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field).toHaveAttribute("aria-expanded", "false");
    for (const name of [
      "aria-autocomplete",
      "aria-controls",
      "aria-activedescendant",
    ])
      expect(field).not.toHaveAttribute(name);
  });

  it("leaves the caret where typing goes on after the picked token", async () => {
    invoke.mockResolvedValue(answer(["src/main.rs"]));
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    const pickInto = async (text: string, caret: number) => {
      await act(async () => {
        fireEvent.change(field, { target: { value: text } });
        field.setSelectionRange(caret, caret);
        fireEvent.keyUp(field);
      });
      await screen.findByRole("option");
      await act(async () => {
        fireEvent.keyDown(field, { key: "Enter" });
      });
      const at = field.selectionStart;
      await type(field, `${field.value.slice(0, at)}x${field.value.slice(at)}`);
      return field.value;
    };
    expect(await pickInto("Read @ma", 8)).toBe("Read @src/main.rs x");
    expect(await pickInto("see @ma then", 7)).toBe("see @src/main.rs xthen");
    expect(await pickInto("see (@ma) then", 8)).toBe(
      "see (@src/main.rs)x then",
    );
  });

  it("does not open for an @ inside a word", async () => {
    render(<Composer />);
    const field = screen.getByRole<HTMLTextAreaElement>("textbox");
    field.focus();
    await type(field, "mail bob@exa");
    expect(invoke).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
