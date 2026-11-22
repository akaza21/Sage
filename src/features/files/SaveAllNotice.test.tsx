import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useWorkbenchStore } from "../../app/store";
import type { ResourceRef } from "../../domain/workbench";
import { SaveAllNotice } from "./SaveAllNotice";

const initialState = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

function report(resources: ResourceRef[]) {
  useWorkbenchStore.setState({
    project: {
      id: "project",
      name: "Project",
      roots: [
        {
          id: "a",
          name: "App",
          ordinal: 0,
          availability: "available",
          message: null,
        },
        {
          id: "b",
          name: "Library",
          ordinal: 1,
          availability: "available",
          message: null,
        },
      ],
    },
    tabs: resources.map((resource, index) => ({
      id: `file-${index}`,
      kind: "file",
      title: resource.relativePath,
      position: index,
      pinned: true,
      dirty: true,
      resource,
    })),
    saveAllReport: {
      projectId: "project",
      failures: resources.map((resource) => ({
        resource,
        message: "Write refused.",
      })),
    },
  });
}

it("summarizes unresolved files compactly, retaining distinct resource identities", () => {
  report([
    { rootId: "a", relativePath: "index.ts" },
    { rootId: "a", relativePath: "index.ts" },
    { rootId: "b", relativePath: "index.ts" },
  ]);
  render(<SaveAllNotice />);
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Save All could not save 2 files: App/index.ts, Library/index.ts.",
  );
  expect(screen.queryByRole("list")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(screen.getAllByRole("listitem")).toHaveLength(2);
  expect(screen.getByRole("button", { name: "App/index.ts" })).toBeVisible();
  expect(
    screen.getByRole("button", { name: "Library/index.ts" }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(useWorkbenchStore.getState().saveAllReport).toBeNull();
});

it("limits the visible summary and reveals unresolved files on demand", () => {
  report(
    Array.from({ length: 7 }, (_, index) => ({
      rootId: "a",
      relativePath: `file-${index}.ts`,
    })),
  );
  render(<SaveAllNotice />);
  expect(screen.getByRole("alert")).toHaveTextContent(
    "Save All could not save 7 files:",
  );
  expect(screen.getByRole("alert")).toHaveTextContent("and 2 more.");
  expect(screen.queryByText(/file-6.ts/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Details" }));
  expect(screen.getAllByRole("listitem")).toHaveLength(7);
  expect(screen.getByRole("button", { name: "App/file-6.ts" })).toBeVisible();
});
