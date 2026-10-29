import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { MixedEolFileView } from "./MixedEolFileView";
import { FileSurface } from "./FileSurface";
import { useWorkbenchStore } from "../../app/store";
import { createInjectedEditorWatchDelivery } from "../../domain/editorWatch";
import type { TextFile } from "../../domain/workbench";

const initial = useWorkbenchStore.getState();

it("coalesces rewrite bursts and cannot resurrect text after a deletion", async () => {
  const resource = { rootId: "root", relativePath: "mixed.txt" };
  const injected = createInjectedEditorWatchDelivery();
  vi.spyOn(getWorkbenchPorts().editor, "watchEditorDocument").mockResolvedValue(
    { resource, delivery: injected.delivery, cancel: async () => undefined },
  );
  let resolve!: (file: TextFile) => void;
  const pending = new Promise<TextFile>((done) => {
    resolve = done;
  });
  const file: TextFile = {
    resource,
    content: "old\r\ntext\n",
    byteLen: 10,
    lineEnding: "crlf",
    modifiedAt: null,
  };
  const read = vi
    .spyOn(getWorkbenchPorts().editor, "readTextFile")
    .mockReturnValueOnce(pending)
    .mockResolvedValue(file);
  render(<MixedEolFileView resource={resource} />);
  await waitFor(() => expect(read).toHaveBeenCalledOnce());
  act(() => {
    injected.emit({
      resource: { ...resource, relativePath: "other.txt" },
      kind: "deleted",
      generation: 1,
    });
    for (let index = 0; index < 100; index += 1)
      injected.emit({ resource, kind: "changed", generation: 1 });
  });
  expect(read).toHaveBeenCalledOnce();
  await act(async () => resolve(file));
  expect(await screen.findByRole("textbox")).toHaveValue("old\ntext\n");
  expect(read).toHaveBeenCalledTimes(2);
  let resolveLate!: (value: TextFile) => void;
  read.mockReturnValueOnce(
    new Promise<TextFile>((done) => {
      resolveLate = done;
    }),
  );
  act(() => injected.emit({ resource, kind: "changed", generation: 1 }));
  expect(screen.queryByRole("textbox")).toBeNull();
  act(() => injected.emit({ resource, kind: "deleted", generation: 1 }));
  await act(async () => resolveLate(file));
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(screen.getByRole("alert")).toHaveTextContent(/deleted/i);
});

it("cancels a watch that finishes binding after the viewer unmounts", async () => {
  const resource = { rootId: "root", relativePath: "mixed.txt" };
  const injected = createInjectedEditorWatchDelivery();
  const cancel = vi.fn(async () => undefined);
  let resolve!: (value: {
    resource: typeof resource;
    delivery: typeof injected.delivery;
    cancel: typeof cancel;
  }) => void;
  const watch = vi
    .spyOn(getWorkbenchPorts().editor, "watchEditorDocument")
    .mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
  const read = vi.spyOn(getWorkbenchPorts().editor, "readTextFile");
  const view = render(<MixedEolFileView resource={resource} />);
  await waitFor(() => expect(watch).toHaveBeenCalledOnce());
  view.unmount();
  await act(async () =>
    resolve({ resource, delivery: injected.delivery, cancel }),
  );
  expect(cancel).toHaveBeenCalledOnce();
  expect(read).not.toHaveBeenCalled();
});

it("explains failed automatic watching and still permits a manual refresh", async () => {
  const resource = { rootId: "root", relativePath: "mixed.txt" };
  vi.spyOn(getWorkbenchPorts().editor, "watchEditorDocument").mockRejectedValue(
    new Error("unavailable"),
  );
  const read = vi
    .spyOn(getWorkbenchPorts().editor, "readTextFile")
    .mockRejectedValue(new Error("missing"));
  render(<MixedEolFileView resource={resource} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("missing");
  expect(
    screen.getByText(/Automatic file watching is unavailable/),
  ).toBeVisible();
  read.mockResolvedValue({
    resource,
    content: "restored\n",
    byteLen: 9,
    lineEnding: "lf",
    modifiedAt: null,
  });
  fireEvent.click(screen.getByRole("button", { name: "Refresh file" }));
  expect(await screen.findByRole("textbox")).toHaveValue("restored\n");
  expect(screen.queryByRole("alert")).toBeNull();
});

it("refreshes external rewrites, invalidates deleted content and cancels its watch", async () => {
  const resource = { rootId: "root", relativePath: "mixed.txt" };
  const injected = createInjectedEditorWatchDelivery();
  const cancel = vi.fn(async () => undefined);
  vi.spyOn(getWorkbenchPorts().editor, "watchEditorDocument").mockResolvedValue(
    {
      resource,
      delivery: injected.delivery,
      cancel,
    },
  );
  const read = vi
    .spyOn(getWorkbenchPorts().editor, "readTextFile")
    .mockResolvedValue({
      resource,
      content: "first\r\nsecond\n",
      byteLen: 14,
      lineEnding: "crlf",
      modifiedAt: null,
    });
  const view = render(<MixedEolFileView resource={resource} />);
  expect(await screen.findByRole("textbox")).toHaveValue("first\nsecond\n");
  read.mockResolvedValue({
    resource,
    content: "rewritten\r\ntext\n",
    byteLen: 16,
    lineEnding: "crlf",
    modifiedAt: null,
  });
  act(() => injected.emit({ resource, kind: "changed", generation: 1 }));
  await waitFor(() =>
    expect(screen.getByRole("textbox")).toHaveValue("rewritten\ntext\n"),
  );
  act(() => injected.emit({ resource, kind: "deleted", generation: 1 }));
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(screen.getByRole("alert")).toHaveTextContent(/deleted/i);
  fireEvent.click(screen.getByRole("button", { name: "Refresh file" }));
  expect(await screen.findByRole("textbox")).toHaveValue("rewritten\ntext\n");
  view.unmount();
  await waitFor(() => expect(cancel).toHaveBeenCalledOnce());
});

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
