import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, expect, it } from "vitest";

import { clearMatches, showMatches } from "./findMarks";

const LINES = 2_000;
const doc = Array.from(
  { length: LINES },
  (_, line) => `line ${line} needle`,
).join("\n");
const matches = (text: string) =>
  [...text.matchAll(/needle/g)].flatMap((match) => [
    match.index,
    match.index + 6,
  ]);

let view: EditorView;
afterEach(() => {
  view.destroy();
  document.body.replaceChildren();
});

function mount() {
  const parent = document.createElement("div");
  document.body.append(parent);
  view = new EditorView({ parent, state: EditorState.create({ doc }) });
  return view;
}

function drawn() {
  const marks = [...view.contentDOM.querySelectorAll(".cm-findMatch")];
  return {
    starts: marks.map((mark) => view.posAtDOM(mark)),
    current: marks
      .filter((mark) => mark.classList.contains("cm-findMatch--current"))
      .map((mark) => view.posAtDOM(mark)),
  };
}

function visibleMatchStarts(ranges: readonly number[]) {
  return ranges
    .filter((_, index) => index % 2 === 0)
    .filter((from) =>
      view.visibleRanges.some((range) => from >= range.from && from < range.to),
    );
}

const scrollTo = (position: number) =>
  view.dispatch({
    effects: EditorView.scrollIntoView(position, { y: "center" }),
  });

it("draws the visible matches, not all of them, and the current one", () => {
  mount();
  const ranges = matches(doc);
  showMatches(view, ranges, 1);

  const { starts, current } = drawn();
  expect(visibleMatchStarts(ranges).length).toBeGreaterThan(1);
  expect(starts).toEqual(expect.arrayContaining(visibleMatchStarts(ranges)));
  expect(starts.length).toBeLessThan(LINES / 4);
  expect(current).toEqual([ranges[2]]);
});

it("marks the newly visible matches as the view scrolls, and the earlier ones again on return", () => {
  mount();
  const ranges = matches(doc);
  showMatches(view, ranges, 0);
  const far = ranges[1_500 * 2]!;
  expect(drawn().starts).not.toContain(far);

  scrollTo(far);
  expect(visibleMatchStarts(ranges)).toContain(far);
  expect(drawn().starts).toEqual(
    expect.arrayContaining(visibleMatchStarts(ranges)),
  );
  expect(drawn().starts).not.toContain(ranges[0]);

  scrollTo(0);
  expect(drawn().starts).toContain(ranges[0]);
  expect(drawn().current).toEqual([ranges[0]]);
});

it("keeps marks on their text through an edit, and drops them all with clearMatches", () => {
  mount();
  showMatches(view, matches(doc), 0);
  view.dispatch({ changes: { from: 0, insert: "// typed\n" } });

  const moved = matches(view.state.doc.toString());
  expect(drawn().starts).toEqual(
    expect.arrayContaining(visibleMatchStarts(moved)),
  );
  expect(drawn().current).toEqual([moved[0]]);

  view.dispatch({ effects: clearMatches() });
  expect(drawn().starts).toEqual([]);
  scrollTo(moved[1_500 * 2]!);
  expect(drawn().starts).toEqual([]);
});
