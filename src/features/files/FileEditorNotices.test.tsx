import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
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
    message: "Disk content changed",
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

const identities = (container: HTMLElement) =>
  [...container.querySelectorAll("[data-notice-identity]")].map((node) =>
    node.getAttribute("data-notice-identity"),
  );

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("FileEditorNotices", () => {
  it("explains a disk conflict once, with its resolutions beside it", async () => {
    setEditor();
    const { container } = render(
      <FileEditorNotices editor={{ document, message: null }} editable />,
    );
    const conflict = await screen.findByRole("group", {
      name: "External change",
    });
    expect(identities(container)).toEqual([
      "file-conflict:4:rootreadme.md:external-change",
    ]);
    expect(conflict).toHaveTextContent("This file changed outside Sage.");
    // The polite region is mounted with the File; a conflict fills it once.
    expect(container.querySelector('[aria-live="polite"]')).toHaveTextContent(
      "File changed outside Sage.",
    );
    expect(screen.queryByText(/Disk content changed/)).toBeNull();
    expect(container.querySelector(".notice")).toHaveClass("notice--warning");
    expect(screen.queryByRole("alert")).toBeNull();
    for (const name of ["Reload", "Keep mine", "Compare"])
      expect(within(conflict).getByRole("button", { name })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Compare" }));
    expect(useWorkbenchStore.getState().editorCompareKey).toContain(
      version.fingerprint,
    );
    expect(screen.getByRole("button", { name: "Compare" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("keeps a conflict and lost recovery storage as two problems", async () => {
    setEditor("the disk is full");
    const { container } = render(
      <FileEditorNotices
        editor={{ document, message: null, unstored: "the disk is full" }}
        editable
      />,
    );
    await screen.findByRole("group", { name: "External change" });
    expect(identities(container)).toEqual([
      "file-conflict:4:rootreadme.md:external-change",
      "file-recovery:4:rootreadme.md",
    ]);
    // The conflict stays a warning; only the lost copy is an alert.
    expect(container.querySelector(".notice")).toHaveClass("notice--warning");
    const recovery = screen.getByRole("alert");
    expect(recovery).toHaveTextContent(
      "Your unsaved changes could not be stored for recovery.",
    );
    fireEvent.click(within(recovery).getByRole("button", { name: "Details" }));
    expect(recovery).toHaveTextContent("the disk is full");
  });

  it("shows a failed save as its own problem", () => {
    const { container } = render(
      <FileEditorNotices
        editor={{
          document: { ...document, conflict: null },
          message: "Couldn't save: the disk is full.",
          failure: 3,
        }}
        editable
      />,
    );
    expect(identities(container)).toEqual(["file-operation:4:rootreadme.md:3"]);
    expect(
      container.querySelector('[aria-live="polite"]'),
    ).toBeEmptyDOMElement();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Couldn't save: the disk is full.",
    );
  });
});
