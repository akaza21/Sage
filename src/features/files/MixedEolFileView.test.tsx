import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { MixedEolFileView } from "./MixedEolFileView";
import { FileSurface } from "./FileSurface";
import { useWorkbenchStore } from "../../app/store";

const initial = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

it("falls back from the native mixed-EOL refusal at the shared File surface", async () => {
  const resource = { rootId: "root", relativePath: "mixed.txt" };
  const tab = {
    id: "file:mixed",
    kind: "file" as const,
    title: "mixed.txt",
    position: 0,
    pinned: false,
    dirty: false,
    resource,
  };
  vi.spyOn(getWorkbenchPorts().editor, "readTextFile").mockResolvedValue({
    resource,
    content: "a\r\nb\n",
    byteLen: 5,
    lineEnding: "crlf",
    modifiedAt: null,
  });
  useWorkbenchStore.setState({
    tabs: [tab],
    activeTabId: tab.id,
    activeEditor: {
      phase: "error",
      resource,
      message:
        "invalid request: mixed LF and CRLF files are not editable in this slice",
    },
    loadEditor: vi.fn(async () => undefined),
  });
  render(<FileSurface tab={tab} />);
  expect(
    await screen.findByRole("textbox", { name: "View mixed.txt" }),
  ).toHaveAttribute("readonly");
  expect(screen.queryByText("Could not open mixed.txt")).toBeNull();
});

it("caps the text view and displays read failures truthfully", async () => {
  const resource = { rootId: "root", relativePath: "large.txt" };
  const read = vi
    .spyOn(getWorkbenchPorts().editor, "readTextFile")
    .mockResolvedValue({
      resource,
      content: "x".repeat(1_048_577),
      byteLen: 1_048_577,
      lineEnding: "none",
      modifiedAt: null,
    });
  const view = render(<MixedEolFileView resource={resource} />);
  expect(
    await screen.findByText("View capped at 1 MiB of text."),
  ).toBeVisible();
  expect(
    (screen.getByRole("textbox") as HTMLTextAreaElement).value.length,
  ).toBe(1_048_576);
  view.unmount();
  read.mockRejectedValue(new Error("File disappeared"));
  render(<MixedEolFileView resource={resource} />);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "File disappeared",
  );
});

it("reads mixed line endings without opening a writable buffer", async () => {
  const resource = { rootId: "root", relativePath: "mixed.ts" };
  const port = getWorkbenchPorts().editor;
  vi.spyOn(port, "readTextFile").mockResolvedValue({
    resource,
    content: "first\r\nsecond\n",
    byteLen: 14,
    lineEnding: "crlf",
    modifiedAt: null,
  });
  const save = vi.spyOn(port, "saveEditorDocument");
  render(<MixedEolFileView resource={resource} />);
  await waitFor(() =>
    expect(screen.getByRole("textbox", { name: "View mixed.ts" })).toHaveValue(
      "first\nsecond\n",
    ),
  );
  expect(screen.getByRole("textbox")).toHaveAttribute("readonly");
  expect(screen.getByText(/Mixed line endings/)).toBeVisible();
  expect(save).not.toHaveBeenCalled();
});
