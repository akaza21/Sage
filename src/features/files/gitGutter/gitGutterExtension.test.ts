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
import { gutterField, setGutterHunks, setGutterPeek } from "./gitGutterState";
import {
  createGutterWorkerCore,
  type GutterRequest,
  type GutterResult,
} from "./gutterWorkerCore";

function fakePool({ manual = false } = {}) {
  const core = createGutterWorkerCore();
  const posts: GutterRequest[] = [];
  const held: (() => void)[] = [];
  let closed = 0;
  const pool: GutterPool = {
    connect(listener: (result: GutterResult) => void) {
      return {
        id: 1,
        post(request) {
          posts.push(request);
          const result = core.handle(request);
          if (!result) return;
          if (manual) held.push(() => listener(result));
          else queueMicrotask(() => listener(result));
        },
        close: () => {
          closed += 1;
        },
      };
    },
  };
  /** Delivers the oldest held result (with `manual`). */
  const answer = () => held.shift()?.();
  return { pool, posts, answer, closed: () => closed, core };
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
    expect(posts.map((post) => post.type)).toEqual(["open", "update"]);
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

describe("git gutter consistency", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  it("never reverts against an old staged text after a stage outside Sage", async () => {
    const view = mount("a\nnew-long\nc\n");
    const { pool } = fakePool({ manual: true });
    const gutter = attachGitGutter(view, pool);
    gutter.setBase("a\nold\nc\n");
    await settle();
    expect(view.state.field(gutterField).hunks).toEqual([]);
    // The stage lands before the first diff answers.
    gutter.setBase("a\nnew-long\nc\n");
    expect(revertHunk(view, 0)).toBe(false);
    expect(view.state.doc.toString()).toBe("a\nnew-long\nc\n");
  });

  it("drops markers computed against the previous staged text", async () => {
    const view = mount("a\nnew\nc\n");
    const { pool, answer } = fakePool({ manual: true });
    const gutter = attachGitGutter(view, pool);
    gutter.setBase("a\nold\nc\n");
    await settle();
    answer();
    expect(kinds(view)).toEqual(["modified"]);
    view.dispatch({ changes: { from: 0, insert: "z" } });
    await settle();
    // Staged while that diff runs: its answer is for the old index.
    gutter.setBase("za\nnew\nc\n");
    expect(kinds(view)).toEqual([]);
    answer();
    expect(kinds(view)).toEqual([]);
    await settle();
    answer();
    expect(kinds(view)).toEqual([]);
    expect(revertHunk(view, 0)).toBe(false);
  });

  it("keeps one request in flight and sends later edits together", async () => {
    const view = mount("a\nb\n");
    const { pool, posts, answer } = fakePool({ manual: true });
    attachGitGutter(view, pool).setBase("a\nb\n");
    await settle();
    for (const offset of [0, 1, 2]) {
      view.dispatch({ changes: { from: offset, insert: "x" } });
      await settle();
    }
    // Still waiting on the open: nothing more was posted.
    expect(posts.map((post) => post.type)).toEqual(["open"]);
    answer();
    await settle();
    expect(posts.map((post) => post.type)).toEqual(["open", "update"]);
    answer();
    expect(kinds(view)).toEqual(["modified"]);
  });

  it("sends the File again when the worker lost it", async () => {
    const view = mount("a\n");
    const { pool, posts, core } = fakePool();
    attachGitGutter(view, pool).setBase("b\n");
    await settle();
    core.handle({ type: "close", id: 1 });
    view.dispatch({ changes: { from: 0, insert: "x" } });
    await settle();
    await settle();
    expect(posts.map((post) => post.type)).toEqual(["open", "update", "open"]);
    expect(kinds(view)).toEqual(["modified"]);
  });
});

describe("git gutter peek and reloads", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  it("keeps an open peek on its change when new results renumber the hunks", async () => {
    const view = mount("a\nb\nc\nd\n");
    const { pool } = fakePool();
    attachGitGutter(view, pool).setBase("a\nb\nc\nd\n");
    await settle();
    view.dispatch({ changes: { from: 6, to: 7, insert: "D" } });
    await settle();
    view.dispatch({ effects: setGutterPeek.of(0) });
    const staged = view.state.field(gutterField).hunks[0]!;
    // A result that lists another change first moves the peek with its change.
    view.dispatch({
      effects: setGutterHunks.of({
        hunks: [{ ...staged, from: 0, to: 1, kind: "added" }, staged],
        truncated: false,
      }),
    });
    expect(view.state.field(gutterField).peek).toBe(1);
  });

  it("drops markers when the whole document is replaced", async () => {
    const view = mount("a\nb\n");
    const { pool } = fakePool();
    attachGitGutter(view, pool).setBase("a\nc\n");
    await settle();
    expect(kinds(view)).toEqual(["modified"]);
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: "x\ny\n" },
    });
    expect(kinds(view)).toEqual([]);
    await settle();
    expect(kinds(view)).toEqual(["modified"]);
  });

  it("drops a pending result when a reload replaces the document mid-diff", async () => {
    const view = mount("a\nB\nc\n");
    const { pool, posts, answer } = fakePool({ manual: true });
    attachGitGutter(view, pool).setBase("a\nb\nc\n");
    await settle();
    expect(posts.map((post) => post.type)).toEqual(["open"]);
    // The reload lands while the worker still holds the diff of "a B c".
    view.dispatch({
      changes: {
        from: 0,
        to: view.state.doc.length,
        insert: "one\ntwo\nthree\n",
      },
    });
    answer();
    expect(kinds(view)).toEqual([]);
    expect(revertHunk(view, 0)).toBe(false);
    expect(view.state.doc.toString()).toBe("one\ntwo\nthree\n");
    await settle();
    answer();
    expect(posts.map((post) => post.type)).toEqual(["open", "update"]);
    expect(view.state.field(gutterField).hunks).toHaveLength(1);
    expect(view.state.field(gutterField).hunks[0]).toMatchObject({
      kind: "modified",
      from: 0,
      to: view.state.doc.length - 1,
    });
  });

  it.each([
    ["the whole document", (length: number) => length],
    ["all but the last break", (length: number) => length - 1],
  ])("drops an in-flight result when a reload replaces %s", async (_, end) => {
    const view = mount("a\nB\nc\n");
    const { pool, answer } = fakePool({ manual: true });
    attachGitGutter(view, pool).setBase("a\nb\nc\n");
    await settle();
    view.dispatch({
      changes: {
        from: 0,
        to: end(view.state.doc.length),
        insert: "one\ntwo\nthree",
      },
    });
    const replaced = view.state.doc.toString();
    answer();
    expect(kinds(view)).toEqual([]);
    expect(revertHunk(view, 0)).toBe(false);
    expect(view.state.doc.toString()).toBe(replaced);
    await settle();
    answer();
    expect(kinds(view).length).toBeGreaterThan(0);
    expect(view.state.field(gutterField).hunks[0]!.from).toBe(0);
  });

  it("drops settled markers an edit reaches past, but keeps them through typing inside", async () => {
    const view = mount("a\nB\nc\nd\n");
    const { pool } = fakePool();
    attachGitGutter(view, pool).setBase("a\nb\nc\nd\n");
    await settle();
    expect(kinds(view)).toEqual(["modified"]);
    view.dispatch({ changes: { from: 3, insert: "x" } });
    expect(view.state.field(gutterField).hunks[0]).toMatchObject({
      from: 2,
      to: 4,
    });
    view.dispatch({ changes: { from: 0, to: 6, insert: "z" } });
    expect(kinds(view)).toEqual([]);
  });

  it("drops a deleted-line marker when text is typed at its anchor", async () => {
    const view = mount("a\nc\n");
    const { pool } = fakePool();
    attachGitGutter(view, pool).setBase("a\nb\nc\n");
    await settle();
    expect(kinds(view)).toEqual(["deleted"]);
    view.dispatch({ changes: { from: 2, insert: "X" } });
    expect(kinds(view)).toEqual([]);
    expect(revertHunk(view, 0)).toBe(false);
    await settle();
    expect(kinds(view)).toEqual(["modified"]);
    expect(revertHunk(view, 0)).toBe(true);
    expect(view.state.doc.toString()).toBe("a\nb\nc\n");
  });
});
