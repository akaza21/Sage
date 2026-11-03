import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { FileEditorHeader } from "./FileEditorHeader";
afterEach(cleanup);
function renderPath(path: string) {
  render(
    createElement(FileEditorHeader, {
      path,
      lineEnding: "lf",
      diskByteLen: 0,
      persisting: false,
      dirty: false,
      saving: false,
      transitioning: false,
    }),
  );
}
describe("file breadcrumb admission through the editor header", () => {
  it("splits a short editor path into VS Code-like chips", () => {
    renderPath("src/features/files/FileSurface.tsx");
    expect(
      [
        ...screen
          .getByRole("navigation", { name: "File path" })
          .querySelectorAll(".file-breadcrumb__item"),
      ].map((item) => item.textContent),
    ).toEqual(["src", "features", "files", "FileSurface.tsx"]);
  });
  it("collapses oversized paths instead of growing an unbounded chip list", () => {
    renderPath("a/b/c/d/e/f/g/train.py");
    expect(
      [
        ...screen
          .getByRole("navigation", { name: "File path" })
          .querySelectorAll(".file-breadcrumb__item"),
      ].map((item) => item.textContent),
    ).toEqual(["a", "…", "e", "f", "g", "train.py"]);
  });
  it.each(["", `${"a/".repeat(4096)}x`])(
    "returns no chips for empty or oversized paths",
    (path) => {
      renderPath(path);
      expect(
        screen.queryByRole("navigation", { name: "File path" }),
      ).toBeNull();
    },
  );
});
