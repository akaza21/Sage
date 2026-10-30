import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FileSurfaceContent } from "./FileSurfaceContent";
import type { EditorConflict } from "../../domain/workbench";

function FileEditorNotices({
  conflict,
  message,
}: {
  conflict: EditorConflict | null;
  message: string | null;
}) {
  const resource = { rootId: "root", relativePath: "readme.md" };
  const version = { fingerprint: "base", byteLen: 0, modifiedAt: null };
  return (
    <FileSurfaceContent
      activeResourceKey="root:readme.md"
      dirty={false}
      editor={{
        phase: "ready",
        resource,
        document: {
          resource,
          disk: version,
          base: version,
          buffer: {
            fingerprint: "buffer",
            byteLen: 0,
            revision: 0,
            content: "",
          },
          lineEnding: "lf",
          dirty: false,
          conflict,
        },
        content: "",
        revision: 0,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message,
      }}
      editorInputRef={{ current: null }}
      find={null}
      goTo={null}
      languageNavigationStatus=""
      path="readme.md"
      previewing
      transitioning={false}
      onCaretOffset={() => {}}
      onEditorChange={() => {}}
      onSurfaceKeyDown={() => {}}
      onTogglePreview={() => {}}
    />
  );
}

afterEach(cleanup);

describe("FileEditorNotices", () => {
  it("lets an external-change conflict own the error surface", () => {
    render(
      <FileEditorNotices
        conflict={{
          kind: "external-change",
          message: "This file changed outside Sage.",
        }}
        message="invalid request: snapshot base no longer matches disk"
      />,
    );

    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByText(/External change detected/)).toBeVisible();
    expect(screen.queryByText(/invalid request/)).toBeNull();
  });

  it("keeps a standalone editor error visible", () => {
    render(<FileEditorNotices conflict={null} message="Save failed." />);

    expect(screen.getByRole("alert")).toHaveTextContent("Save failed.");
  });
});
