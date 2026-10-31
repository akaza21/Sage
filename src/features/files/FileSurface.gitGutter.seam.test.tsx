import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { useWorkbenchStore } from "../../app/store";
import type {
  EditorDocument,
  FileGitBase,
  RootRepositoryStatus,
  WorkbenchTab,
} from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { FileSurface } from "./FileSurface";

// jsdom has no Worker: the shared pool runs the worker's core in-process.
vi.mock("./gitGutter/gutterWorkerClient", async () => {
  const { createGutterWorkerCore } =
    await import("./gitGutter/gutterWorkerCore");
  const core = createGutterWorkerCore();
  let nextId = 1;
  return {
    sharedGutterWorkerPool: () => ({
      connect(listener: (result: unknown) => void) {
        const id = nextId++;
        return {
          id,
          post(request: Parameters<typeof core.handle>[0]) {
            const result = core.handle(request);
            if (result) queueMicrotask(() => listener(result));
          },
          close: () => core.handle({ type: "close", id }),
        };
      },
    }),
  };
});

const resource = { rootId: "root-a", relativePath: "gutter.txt" };
const version = { fingerprint: "disk", byteLen: 9, modifiedAt: null };
const document: EditorDocument = {
  resource,
  disk: version,
  base: version,
  buffer: {
    fingerprint: "disk",
    byteLen: 9,
    revision: 0,
    content: "one\nTWO\n",
  },
  lineEnding: "lf",
  dirty: false,
  conflict: null,
};
const tab: WorkbenchTab = {
  id: "file:gutter.txt",
  kind: "file",
  title: "gutter.txt",
  position: 1,
  pinned: false,
  dirty: false,
  resource,
};
const root = (): RootRepositoryStatus => ({
  rootId: "root-a",
  state: "repository",
  branch: "main",
  changes: [],
  truncated: false,
  elapsedMs: 1,
  message: null,
});
const initialState = useWorkbenchStore.getState();
const initialSettings = useEditorSettingsStore.getState();

function setReady(state: RootRepositoryStatus["state"] = "repository") {
  useWorkbenchStore.setState({
    tabs: [tab],
    activeTabId: tab.id,
    repositories: {
      roots: [{ ...root(), state }],
      truncated: false,
      elapsedMs: 1,
    },
    activeEditor: {
      phase: "ready",
      resource,
      document,
      content: document.buffer.content,
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    },
  });
}

function stubBase() {
  return vi
    .spyOn(getWorkbenchPorts().repository, "getFileGitBase")
    .mockImplementation(async (_resource, knownOid): Promise<FileGitBase> =>
      knownOid === "oid-1"
        ? { state: "unchanged", oid: "oid-1" }
        : { state: "ready", oid: "oid-1", content: "one\ntwo\n" },
    );
}

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  useEditorSettingsStore.setState(initialSettings, true);
  vi.restoreAllMocks();
});

describe("FileSurface change markers (EP-14)", () => {
  it("marks an Editor File's changes and reloads the staged text on a status change", async () => {
    const getBase = stubBase();
    setReady();
    const { container } = render(<FileSurface tab={tab} />);
    await screen.findByRole("textbox", { name: "Edit gutter.txt" });
    await waitFor(
      () =>
        expect(
          container.querySelector(".git-gutter-marker--modified"),
        ).not.toBeNull(),
      { timeout: 2_000 },
    );
    expect(getBase).toHaveBeenCalledWith(resource, null);

    // EP-12 publishes a new root object when Git state changed.
    useWorkbenchStore.setState((state) => ({
      repositories: { ...state.repositories, roots: [root()] },
    }));
    await waitFor(() => expect(getBase).toHaveBeenCalledTimes(2));
    expect(getBase).toHaveBeenLastCalledWith(resource, "oid-1");

    // Staging another hunk can leave the status equal (EP-12 then keeps the
    // root object); the finished refresh still reloads the staged text.
    useWorkbenchStore.setState({ repositoryRefreshing: true });
    useWorkbenchStore.setState({ repositoryRefreshing: false });
    await waitFor(() => expect(getBase).toHaveBeenCalledTimes(3));
  });

  it("gives the Agent pane's read-only File no gutter and asks Git nothing", async () => {
    const getBase = stubBase();
    setReady();
    const { container } = render(<FileSurface tab={tab} readOnly />);
    await screen.findByRole("textbox", { name: "Edit gutter.txt" });
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(container.querySelector(".git-gutter")).toBeNull();
    expect(getBase).not.toHaveBeenCalled();
  });

  it.each([
    ["the setting is off", "repository", false],
    ["the root is not a repository", "notRepository", true],
  ] as const)("adds no gutter when %s", async (_name, state, on) => {
    const getBase = stubBase();
    useEditorSettingsStore.setState((current) => ({
      settings: { ...current.settings, "editor.gitGutter": on },
    }));
    setReady(state);
    const { container } = render(<FileSurface tab={tab} />);
    await screen.findByRole("textbox", { name: "Edit gutter.txt" });
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(container.querySelector(".git-gutter")).toBeNull();
    expect(getBase).not.toHaveBeenCalled();
  });
});
