import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { TransactionSpec } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

import { useWorkbenchStore } from "../../app/store";
import { resourceKey } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import { registerReplaceEditor } from "../files/editorTextReplace";
import type { WorkspaceReplaceGateway } from "./workspaceReplace";
import type { WorkspaceSearchGateway } from "./workspaceSearchDomain";
import { WorkspaceSearchView } from "./WorkspaceSearchView";

const initialStore = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialStore, true);
});

const alpha: ResourceRef = { rootId: "root-a", relativePath: "src/alpha.ts" };
const beta: ResourceRef = { rootId: "root-a", relativePath: "src/beta.ts" };

function searchGateway(query = "oldName"): WorkspaceSearchGateway {
  return {
    search: vi.fn().mockResolvedValue({
      requestId: "request-a",
      projectId: "project-a",
      transitionEpoch: 4,
      indexGeneration: 0,
      query,
      matches: [
        {
          resource: alpha,
          lineNumber: 1,
          columnNumber: 7,
          preview: "const oldName = 1;",
        },
        {
          resource: alpha,
          lineNumber: 2,
          columnNumber: 1,
          preview: "oldName();",
        },
        {
          resource: beta,
          lineNumber: 3,
          columnNumber: 5,
          preview: "use(oldName);",
        },
      ],
      roots: [
        {
          rootId: "root-a",
          rootName: "Sage",
          state: "complete",
          filesScanned: 2,
          bytesScanned: 64,
          entriesVisited: 2,
          skippedFiles: 0,
          errorCount: 0,
          message: null,
        },
      ],
      truncated: false,
      continuationToken: null,
      limitReasons: [],
      elapsedMs: 1,
    }),
  };
}

function replaceGateway(): WorkspaceReplaceGateway {
  return {
    preview: vi.fn().mockImplementation(({ resources, only }) =>
      Promise.resolve({
        files: (resources as ResourceRef[])
          .filter((file) => file.relativePath !== "src/beta.ts" || !only)
          .map((file) => ({
            resource: file,
            fingerprint: `fp-${file.relativePath}`,
            count: only ? 1 : file === alpha ? 2 : 1,
            lines: [
              {
                lineNumber: only?.lineNumber ?? 1,
                columnNumber: only?.columnNumber ?? 7,
                // The line Search showed for that match.
                ...(only?.lineNumber === 2
                  ? { before: "oldName();", after: "newName();" }
                  : {
                      before: "const oldName = 1;",
                      after: "const newName = 1;",
                    }),
              },
            ],
            linesTruncated: false,
          })),
        problems: [],
        truncated: false,
      }),
    ),
    apply: vi.fn().mockImplementation(({ files }) =>
      Promise.resolve({
        replaced: (files as { resource: ResourceRef }[])
          .filter((file) => file.resource.relativePath !== "src/beta.ts")
          .map((file) => ({
            resource: file.resource,
            count: 2,
            fingerprint: "fp-next",
          })),
        problems: (files as { resource: ResourceRef }[])
          .filter((file) => file.resource.relativePath === "src/beta.ts")
          .map((file) => ({
            resource: file.resource,
            kind: "stale",
            message: "it changed on disk since the search; search again",
          })),
      }),
    ),
    replaceText: vi.fn().mockImplementation(({ text }) =>
      Promise.resolve({
        content: (text as string).replaceAll("oldName", "newName"),
        count: 1,
        lines: [
          {
            lineNumber: 1,
            columnNumber: 1,
            before: "oldName",
            after: "newName",
          },
        ],
        linesTruncated: false,
      }),
    ),
  };
}

async function searchAndOpenReplace(
  replace: WorkspaceReplaceGateway,
  search = searchGateway(),
  query = "oldName",
) {
  render(
    <WorkspaceSearchView
      projectId="project-a"
      transitionEpoch={4}
      indexGeneration={0}
      gateway={search}
      replaceGateway={replace}
      onOpenMatch={vi.fn()}
    />,
  );
  fireEvent.change(screen.getByLabelText("Search text"), {
    target: { value: query },
  });
  fireEvent.submit(screen.getByLabelText("Search text").closest("form")!);
  await screen.findByRole("button", {
    name: "Open src/alpha.ts at line 1, column 7",
  });
  expect(screen.queryByLabelText("Replace text")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Toggle Replace" }));
  // Replace loads on demand, so its row shows once its chunk is in.
  fireEvent.change(await screen.findByLabelText("Replace text"), {
    target: { value: "newName" },
  });
  return search;
}

describe("Workspace Replace", () => {
  it("opens from Ctrl+H and keeps its text while closed", async () => {
    await searchAndOpenReplace(replaceGateway());
    const toggle = screen.getByRole("button", { name: "Toggle Replace" });
    expect(screen.getByLabelText("Replace text")).toHaveFocus();
    fireEvent.click(toggle);
    expect(screen.queryByLabelText("Replace text")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Replace all in src/alpha.ts" }),
    ).toBeNull();
    fireEvent.keyDown(screen.getByLabelText("Search text"), {
      key: "h",
      ctrlKey: true,
    });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Replace text")).toHaveValue("newName");
    expect(
      screen.getByRole("button", { name: "Replace all in src/alpha.ts" }),
    ).toBeVisible();
  });

  it("sends the regex mode with the Search and keeps it in the Replace", async () => {
    const replace = replaceGateway();
    const search = await searchAndOpenReplace(
      replace,
      searchGateway("old(\\w+)"),
      "old(\\w+)",
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Use regular expression" }),
    );
    fireEvent.submit(screen.getByLabelText("Search text").closest("form")!);
    await waitFor(() =>
      expect(search.search).toHaveBeenLastCalledWith(
        expect.objectContaining({ useRegex: true }),
        expect.anything(),
      ),
    );
    await screen.findByRole("button", {
      name: "Open src/alpha.ts at line 1, column 7",
    });
    expect(screen.getByLabelText("Replace text")).toHaveAttribute(
      "placeholder",
      "Replace ($1 for a group)",
    );
    fireEvent.change(screen.getByLabelText("Replace text"), {
      target: { value: "new$1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Preserve case" }));
    fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
    await screen.findByRole("dialog");
    expect(replace.preview).toHaveBeenCalledWith(
      expect.objectContaining({
        spec: {
          query: "old(\\w+)",
          replacement: "new$1",
          caseSensitive: false,
          wholeWord: false,
          useRegex: true,
          preserveCase: true,
        },
      }),
    );
  });

  it("previews Replace all with match and file counts before writing", async () => {
    const replace = replaceGateway();
    const search = await searchAndOpenReplace(replace);

    fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("heading", {
        name: "Replace 3 matches in 2 files?",
      }),
    ).toBeVisible();
    expect(replace.apply).not.toHaveBeenCalled();
    const changes = within(dialog).getByRole("list", {
      name: "Changes to make",
    });
    expect(
      within(changes).getByRole("listitem", {
        name: "src/alpha.ts: 2 matches",
      }),
    ).toBeVisible();
    expect(within(changes).getAllByText("const oldName = 1;")[0].tagName).toBe(
      "DEL",
    );
    expect(within(changes).getAllByText("const newName = 1;")[0].tagName).toBe(
      "INS",
    );
    expect(replace.preview).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "project-a",
        expectedTransitionEpoch: 4,
        resources: [alpha, beta],
      }),
    );

    fireEvent.click(within(dialog).getByRole("button", { name: "Replace" }));
    expect(
      await screen.findByText("Replaced 2 matches in 1 file."),
    ).toBeVisible();
    expect(replace.apply).toHaveBeenCalledWith(
      expect.objectContaining({
        files: [
          { resource: alpha, fingerprint: "fp-src/alpha.ts" },
          { resource: beta, fingerprint: "fp-src/beta.ts" },
        ],
      }),
    );
    // The stale File is named, not written.
    expect(
      within(
        screen.getByRole("list", { name: "Files left as they are" }),
      ).getByText(
        "src/beta.ts: it changed on disk since the search; search again",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    // Search runs again, so results show the files as they now are.
    await waitFor(() => expect(search.search).toHaveBeenCalledTimes(2));
  });

  it("replaces one match, or all in one File, and cancels without writing", async () => {
    const replace = replaceGateway();
    await searchAndOpenReplace(replace);

    fireEvent.click(
      screen.getByRole("button", {
        name: "Replace the match at line 2, column 1 in src/alpha.ts",
      }),
    );
    let dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("heading", {
        name: "Replace 1 match in 1 file?",
      }),
    ).toBeVisible();
    expect(replace.preview).toHaveBeenLastCalledWith(
      expect.objectContaining({
        resources: [alpha],
        only: { lineNumber: 2, columnNumber: 1 },
      }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Replace" }));
    await screen.findByText("Replaced 2 matches in 1 file.");
    expect(replace.apply).toHaveBeenLastCalledWith(
      expect.objectContaining({
        files: [
          {
            resource: alpha,
            fingerprint: "fp-src/alpha.ts",
            only: { lineNumber: 2, columnNumber: 1 },
          },
        ],
      }),
    );

    await screen.findByRole("button", {
      name: "Open src/alpha.ts at line 1, column 7",
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Replace all in src/beta.ts" }),
    );
    dialog = await screen.findByRole("dialog");
    expect(replace.preview).toHaveBeenLastCalledWith(
      expect.objectContaining({ resources: [beta] }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(replace.apply).toHaveBeenCalledTimes(1);
  });

  it("names Files the preview refused and reports a preview failure", async () => {
    const replace = replaceGateway();
    vi.mocked(replace.preview)
      .mockResolvedValueOnce({
        files: [],
        problems: [
          {
            resource: alpha,
            kind: "refused",
            message: "Sage doesn't replace in protected folders such as .git",
          },
        ],
        truncated: false,
      })
      .mockRejectedValueOnce(
        new Error("invalid request: the regular expression is invalid"),
      );
    await searchAndOpenReplace(replace);

    fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("heading", { name: "Nothing to replace" }),
    ).toBeVisible();
    expect(
      within(dialog).getByText(
        "src/alpha.ts: Sage doesn't replace in protected folders such as .git",
      ),
    ).toBeVisible();
    expect(
      within(dialog).queryByRole("button", { name: "Replace" }),
    ).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));

    fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
    expect(
      await screen.findByText(
        "Couldn't preview the Replace: the regular expression is invalid",
      ),
    ).toBeVisible();
    expect(replace.apply).not.toHaveBeenCalled();
  });

  it("holds a replacement to the native limit in UTF-8 bytes", async () => {
    const replace = replaceGateway();
    await searchAndOpenReplace(replace);
    const field = screen.getByLabelText("Replace text");
    // 1,025 characters, under the field's length, but 2,050 bytes.
    fireEvent.change(field, { target: { value: "é".repeat(1025) } });

    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByRole("button", { name: "Replace all" })).toBeDisabled();
    fireEvent.click(
      screen.getByRole("button", { name: "Replace all in src/alpha.ts" }),
    );
    expect(
      await screen.findByText("The replacement is longer than 2048 bytes."),
    ).toBeVisible();
    expect(replace.preview).not.toHaveBeenCalled();
  });

  it("changes a File with unsaved edits in its editor, not on disk", async () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          ...initialStore.tabs[0],
          id: "file:alpha",
          kind: "file",
          resource: alpha,
          dirty: true,
        },
      ],
      activeEditor: {
        ...(initialStore.activeEditor ?? {}),
        phase: "ready",
        resource: alpha,
        content: "oldName();\n",
      },
    } as unknown as Partial<ReturnType<typeof useWorkbenchStore.getState>>);
    const view = new EditorView({ doc: "oldName();\n" });
    const changes: string[] = [];
    const dispatch = view.dispatch.bind(view);
    vi.spyOn(view, "dispatch").mockImplementation((...specs) => {
      const transaction = specs[0] as TransactionSpec;
      changes.push(JSON.stringify(transaction.changes));
      dispatch(transaction);
    });
    const unregister = registerReplaceEditor(resourceKey(alpha), view);
    try {
      const replace = replaceGateway();
      await searchAndOpenReplace(replace);
      fireEvent.click(
        screen.getByRole("button", { name: "Replace all in src/alpha.ts" }),
      );
      const dialog = await screen.findByRole("dialog");
      expect(within(dialog).getByText("1 match · unsaved edits")).toBeVisible();
      expect(replace.preview).not.toHaveBeenCalled();
      expect(replace.replaceText).toHaveBeenCalledWith(
        expect.objectContaining({ text: "oldName();\n" }),
      );
      fireEvent.click(within(dialog).getByRole("button", { name: "Replace" }));
      await screen.findByText("Replaced 1 match in 1 file.");
      // Only what differs is replaced, so the rest of the text keeps its
      // marks and the cursor.
      expect(changes).toEqual([
        JSON.stringify({ from: 0, to: 3, insert: "new" }),
      ]);
      expect(view.state.doc.toString()).toBe("newName();\n");
      expect(replace.apply).not.toHaveBeenCalled();
    } finally {
      unregister();
      view.destroy();
    }
  });
});
