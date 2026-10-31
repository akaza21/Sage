import { undo } from "@codemirror/commands";
import { EditorState, Text } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { history } from "@codemirror/commands";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  GUTTER_IDLE_MS,
  attachGitGutter,
  revertHunk,
  type GutterPool,
} from "./gitGutterExtension";
import { gutterField, setGutterPeek } from "./gitGutterState";
import {
  createGutterWorkerCore,
  type GutterRequest,
  type GutterResult,
} from "./gutterWorkerCore";

function fakePool() {
  const core = createGutterWorkerCore();
  const posts: GutterRequest[] = [];
  let closed = 0;
  const pool: GutterPool = {
    connect(listener: (result: GutterResult) => void) {
      return {
        id: 1,
        post(request) {
          posts.push(request);
          const result = core.handle(request);
          if (result) queueMicrotask(() => listener(result));
        },
        close: () => {
          closed += 1;
        },
      };
    },
  };
  return { pool, posts, closed: () => closed };
}

function mount(doc: string, readOnly = false) {
  const parent = document.createElement("div");
  document.body.append(parent);
  return new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [history(), EditorState.readOnly.of(readOnly)],
    }),
  });
}

async function settle() {
  await vi.advanceTimersByTimeAsync(GUTTER_IDLE_MS);
}

const kinds = (view: EditorView) =>
  view.state.field(gutterField).hunks.map((hunk) => hunk.kind);

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe("git gutter", () => {
  it("marks added, modified and deleted lines once typing pauses", async () => {
    const view = mount("a\nb\nc\nd\n");
    const { pool, posts } = fakePool();
    const gutter = attachGitGutter(view, pool);
    gutter.setBase("a\nb\nc\nd\n");
    await settle();
    expect(posts.map((post) => post.type)).toEqual(["open"]);
    expect(kinds(view)).toEqual([]);

    view.dispatch({ changes: { from: 0, insert: "new\n" } });
    view.dispatch({ changes: { from: 6, to: 7, insert: "B" } });
    const last = view.state.doc.line(5);
    view.dispatch({ changes: { from: last.from, to: last.to + 1 } });
    expect(posts).toHaveLength(1);
    await settle();
    // One message for the three edits, carrying only the changes.
    expect(posts.map((post) => post.type)).toEqual(["open", "edit"]);
    expect(kinds(view)).toEqual(["added", "modified", "deleted"]);
    expect(view.dom.querySelector(".git-gutter")).not.toBeNull();
    gutter.destroy();
  });

  it("waits for a pause and never stringifies the document per keystroke", async () => {
    const view = mount("x\n".repeat(5_000));
    const { pool, posts } = fakePool();
    attachGitGutter(view, pool).setBase("x\n".repeat(5_000));
    await settle();
    const toString = vi.spyOn(Text.prototype, "toString");
    for (let index = 0; index < 200; index += 1) {
      view.dispatch({ changes: { from: index, insert: "y" } });
      vi.advanceTimersByTime(GUTTER_IDLE_MS - 1);
    }
    expect(posts).toHaveLength(1);
    await settle();
    expect(posts).toHaveLength(2);
    expect(toString).not.toHaveBeenCalled();
    expect(kinds(view)).toEqual(["modified"]);
  });

  it("keeps markers on their lines through edits made before the next diff", async () => {
    const view = mount("a\nb\n");
    const { pool } = fakePool();
    attachGitGutter(view, pool).setBase("a\nb\n");
    await settle();
    view.dispatch({ changes: { from: 2, to: 3, insert: "B" } });
    await settle();
    view.dispatch({ changes: { from: 0, insert: "zz\n" } });
    const [hunk] = view.state.field(gutterField).hunks;
    expect(view.state.doc.lineAt(hunk!.from).text).toBe("B");
  });

  it.each([
    [
      "a modified line",
      "a\nb\nc\n",
      (v: EditorView) =>
        v.dispatch({ changes: { from: 2, to: 3, insert: "B" } }),
    ],
    [
      "added lines at the end",
      "a\nb",
      (v: EditorView) => v.dispatch({ changes: { from: 3, insert: "\nc\nd" } }),
    ],
    [
      "added lines at the start",
      "a\nb\n",
      (v: EditorView) => v.dispatch({ changes: { from: 0, insert: "x\n" } }),
    ],
    [
      "deleted first line",
      "a\nb\nc\n",
      (v: EditorView) => v.dispatch({ changes: { from: 0, to: 2 } }),
    ],
    [
      "deleted last lines",
      "a\nb\nc",
      (v: EditorView) => v.dispatch({ changes: { from: 1, to: 5 } }),
    ],
    [
      "a removed final line break",
      "a\nb\n",
      (v: EditorView) => v.dispatch({ changes: { from: 3, to: 4 } }),
    ],
  ])("reverts %s in one undo step", async (_name, base, edit) => {
    const view = mount(base);
    const { pool } = fakePool();
    attachGitGutter(view, pool).setBase(base);
    await settle();
    edit(view);
    await settle();
    const edited = view.state.doc.toString();
    expect(view.state.field(gutterField).hunks).toHaveLength(1);
    expect(revertHunk(view, 0)).toBe(true);
    expect(view.state.doc.toString()).toBe(base);
    await settle();
    expect(kinds(view)).toEqual([]);
    undo(view);
    expect(view.state.doc.toString()).toBe(edited);
  });

  it("peeks at the staged lines and reverts from the peek", async () => {
    const view = mount("a\nold\nc\n");
    const { pool } = fakePool();
    attachGitGutter(view, pool).setBase("a\nold\nc\n");
    await settle();
    view.dispatch({ changes: { from: 2, to: 5, insert: "new" } });
    await settle();
    view.dispatch({ effects: setGutterPeek.of(0) });
    const peek = view.dom.querySelector(".git-gutter-peek");
    expect(peek?.getAttribute("aria-label")).toBe("Change 1 of 1");
    expect(peek?.querySelector(".git-gutter-peek__line")?.textContent).toBe(
      "old",
    );
    peek
      ?.querySelector<HTMLButtonElement>('[aria-label="Revert change"]')
      ?.click();
    expect(view.state.doc.toString()).toBe("a\nold\nc\n");
    expect(view.dom.querySelector(".git-gutter-peek")).toBeNull();
  });

  it("offers no revert in a read-only editor", async () => {
    const view = mount("a\nb\n", true);
    const { pool } = fakePool();
    attachGitGutter(view, pool).setBase("a\nc\n");
    await settle();
    expect(kinds(view)).toEqual(["modified"]);
    view.dispatch({ effects: setGutterPeek.of(0) });
    expect(view.dom.querySelector('[aria-label="Revert change"]')).toBeNull();
    expect(revertHunk(view, 0)).toBe(false);
  });

  it("drops markers when the File loses its staged text, and on destroy", async () => {
    const view = mount("a\n");
    const { pool, posts, closed } = fakePool();
    const gutter = attachGitGutter(view, pool);
    gutter.setBase("b\n");
    await settle();
    expect(kinds(view)).toEqual(["modified"]);
    gutter.setBase(null);
    expect(kinds(view)).toEqual([]);
    expect(posts.at(-1)?.type).toBe("close");
    gutter.destroy();
    expect(closed()).toBe(1);
    expect(view.state.field(gutterField, false)).toBeUndefined();
    expect(view.dom.querySelector(".git-gutter")).toBeNull();
  });
});

describe("git gutter opening", () => {
  it("opens with the text the caller holds instead of building it", async () => {
    vi.useFakeTimers();
    const view = mount("held\n");
    const { pool, posts } = fakePool();
    const toString = vi.spyOn(Text.prototype, "toString");
    attachGitGutter(view, pool, () => "held\n").setBase("held\n");
    await settle();
    expect(posts[0]).toMatchObject({ type: "open", doc: "held\n" });
    expect(toString).not.toHaveBeenCalled();
    vi.useRealTimers();
  });
});
