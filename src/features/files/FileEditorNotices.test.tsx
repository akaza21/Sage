import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { EditorDocument } from "../../domain/workbench";
import { FileEditorNotices } from "./FileEditorNotices";

const resource = { rootId: "root", relativePath: "readme.md" };
const version = { fingerprint: "base", byteLen: 0, modifiedAt: null };
const document: EditorDocument = {
  resource,
  disk: version,
  base: version,
  buffer: { fingerprint: "buffer", byteLen: 0, revision: 0, content: "" },
  lineEnding: "lf",
  dirty: true,
  conflict: {
    kind: "external-change",
    message: "This file changed outside Sage.",
  },
};
const initialState = useWorkbenchStore.getState();

function setEditor(unstored?: string) {
  useWorkbenchStore.setState({
    activeEditor: {
      phase: "ready",
      resource,
      document,
      content: "",
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
      unstored,
    },
  });
}

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("FileEditorNotices", () => {
  it("shows one disk-change explanation with working resolution controls", async () => {
    setEditor();
    const { container } = render(
      <FileEditorNotices
        document={document}
        editable
        message="invalid request: snapshot base no longer matches disk"
      />,
    );
    expect(
      await screen.findByText("This file changed outside Sage."),
    ).toBeVisible();
    expect(container.querySelectorAll(".notice")).toHaveLength(1);
    expect(container.querySelector(".notice")).toHaveClass("notice--warning");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(/invalid request/)).toBeNull();
    expect(screen.getByRole("button", { name: "Keep mine" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Details" }));
    expect(screen.getByRole("button", { name: "Reload" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Compare" }));
    expect(useWorkbenchStore.getState().editorCompareKey).toContain(
      version.fingerprint,
    );
    expect(screen.getByRole("button", { name: "Compare" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("keeps conflict and lost recovery storage distinct even with identical diagnostic text", async () => {
    const diagnostic = document.conflict!.message;
    setEditor(diagnostic);
    const { container } = render(
      <FileEditorNotices
        document={document}
        editable
        message={diagnostic}
        messageKind="recovery"
        unstored={diagnostic}
      />,
    );
    expect(
      await screen.findByText("This file changed outside Sage."),
    ).toBeVisible();
    const problems = [...container.querySelectorAll("[data-notice-identity]")];
    expect(problems).toHaveLength(2);
    expect(
      new Set(problems.map((node) => node.getAttribute("data-notice-identity")))
        .size,
    ).toBe(2);
    expect(screen.getAllByRole("alert")).toHaveLength(2);
    expect(
      screen.getByText(
        "Your unsaved changes could not be stored for recovery.",
      ),
    ).toBeVisible();
  });

  it("keeps a separate failed operation visible even when its text matches the conflict", async () => {
    setEditor();
    const { container } = render(
      <FileEditorNotices
        document={document}
        editable
        message={document.conflict!.message}
        messageKind="operation"
      />,
    );
    expect(
      await screen.findAllByText("This file changed outside Sage."),
    ).toHaveLength(2);
    expect(container.querySelectorAll(".notice")).toHaveLength(2);
  });

  it("keeps a standalone editor error visible", () => {
    render(
      <FileEditorNotices
        document={{ ...document, conflict: null }}
        editable
        message="Save failed."
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Save failed.");
  });

  it("shows keeping the local version as information", () => {
    const { container } = render(
      <FileEditorNotices
        document={{ ...document, conflict: null }}
        editable
        message={null}
        keptLocal
      />,
    );
    expect(screen.getByText(/Your local version is kept/)).toBeVisible();
    expect(container.querySelector(".notice")).toHaveClass("notice--info");
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
