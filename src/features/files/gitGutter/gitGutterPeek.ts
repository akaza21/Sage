import { WidgetType, type EditorView } from "@codemirror/view";

/** Staged lines drawn in the peek; the rest are counted, as VS Code caps its peek height. */
export const MAX_PEEK_LINES = 200;

export interface PeekActions {
  previous: (view: EditorView) => void;
  next: (view: EditorView) => void;
  revert: (view: EditorView) => void;
  close: (view: EditorView) => void;
}

const ICONS: Record<keyof PeekActions, readonly string[]> = {
  previous: ["m18 15-6-6-6 6"],
  next: ["m6 9 6 6 6-6"],
  revert: ["M9 14 4 9l5-5", "M4 9h10.5a5.5 5.5 0 0 1 0 11H11"],
  close: ["M18 6 6 18", "m6 6 12 12"],
};

const LABELS: Record<keyof PeekActions, string> = {
  previous: "Previous change",
  next: "Next change",
  revert: "Revert change",
  close: "Close",
};

function icon(paths: readonly string[]) {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  for (const [name, value] of [
    ["viewBox", "0 0 24 24"],
    ["width", "14"],
    ["height", "14"],
    ["fill", "none"],
    ["stroke", "currentColor"],
    ["stroke-width", "2"],
    ["stroke-linecap", "round"],
    ["stroke-linejoin", "round"],
    ["aria-hidden", "true"],
  ])
    svg.setAttribute(name!, value!);
  for (const d of paths) {
    const path = document.createElementNS(ns, "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  return svg;
}

/**
 * VS Code's dirty-diff peek: the staged lines a change replaced, under the
 * change, with Previous, Next, Revert and Close.
 */
export class GitGutterPeek extends WidgetType {
  constructor(
    private readonly index: number,
    private readonly total: number,
    /** Staged text the change replaced; null for added lines. */
    private readonly staged: string | null,
    private readonly canRevert: boolean,
    private readonly actions: PeekActions,
  ) {
    super();
  }

  override eq(other: GitGutterPeek) {
    return (
      other.index === this.index &&
      other.total === this.total &&
      other.staged === this.staged &&
      other.canRevert === this.canRevert
    );
  }

  override toDOM(view: EditorView) {
    const root = document.createElement("div");
    root.className = "git-gutter-peek";
    root.setAttribute("role", "region");
    const title = `Change ${this.index + 1} of ${this.total}`;
    root.setAttribute("aria-label", title);
    const header = document.createElement("div");
    header.className = "git-gutter-peek__header";
    const heading = document.createElement("span");
    heading.className = "git-gutter-peek__title";
    heading.textContent = title;
    const toolbar = document.createElement("div");
    toolbar.className = "git-gutter-peek__actions";
    toolbar.setAttribute("role", "toolbar");
    toolbar.setAttribute("aria-label", "Change actions");
    for (const action of ["previous", "next", "revert", "close"] as const) {
      if (action === "revert" && !this.canRevert) continue;
      const button = document.createElement("button");
      button.type = "button";
      button.className = `git-gutter-peek__action git-gutter-peek__action--${action}`;
      button.title = LABELS[action];
      button.setAttribute("aria-label", LABELS[action]);
      button.disabled =
        action !== "revert" && action !== "close" && this.total < 2;
      button.append(icon(ICONS[action]));
      // Keeps the editor's selection; the click runs the action.
      button.addEventListener("mousedown", (event) => event.preventDefault());
      button.addEventListener("click", () => this.actions[action](view));
      toolbar.append(button);
    }
    header.append(heading, toolbar);
    root.append(header);

    const lines = this.staged?.split("\n") ?? [];
    const body = document.createElement("div");
    body.className = "git-gutter-peek__lines";
    if (this.staged === null) {
      const note = document.createElement("div");
      note.className = "git-gutter-peek__note";
      note.textContent = "Added lines; nothing staged here.";
      body.append(note);
    } else {
      for (const text of lines.slice(0, MAX_PEEK_LINES)) {
        const line = document.createElement("div");
        line.className = "git-gutter-peek__line";
        line.textContent = text || "​";
        body.append(line);
      }
      if (lines.length > MAX_PEEK_LINES) {
        const more = document.createElement("div");
        more.className = "git-gutter-peek__note";
        more.textContent = `${(lines.length - MAX_PEEK_LINES).toLocaleString()} more staged lines`;
        body.append(more);
      }
    }
    root.append(body);
    return root;
  }

  override ignoreEvent() {
    return true;
  }
}
