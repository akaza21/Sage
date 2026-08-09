import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () => <div>Chat surface</div>,
}));
vi.mock("../features/diff/DiffSurface", () => ({
  DiffSurface: () => <div>Diff surface</div>,
}));
vi.mock("../features/files/FileSurface", () => ({
  FileSurface: () => <div>File surface</div>,
}));
vi.mock("./DirtyTabDialog", () => ({ DirtyTabDialog: () => null }));
vi.mock("./LeftSidebar", () => ({ LeftSidebar: () => null }));
vi.mock("./RightSidebar", () => ({ RightSidebar: () => null }));
vi.mock("./StatusBar", () => ({ StatusBar: () => null }));
vi.mock("./TabStrip", () => ({ TabStrip: () => null }));
vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => false,
}));

import { Workbench } from "./Workbench";

const initialState = useWorkbenchStore.getState();
const resource = { rootId: "root-a", relativePath: "src/file.txt" };

function tab(kind: WorkbenchTab["kind"]): WorkbenchTab {
  return {
    id: `${kind}:src/file.txt`,
    kind,
    title: "file.txt",
    position: 0,
    pinned: false,
    dirty: false,
    resource: kind === "chat" ? undefined : resource,
  };
}

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

describe("Workbench repository mutation errors", () => {
  it.each(["file", "diff"] as const)(
    "renders the exact Source Control error on the active %s surface",
    (kind) => {
      const activeTab = tab(kind);
      useWorkbenchStore.setState({
        tabs: [activeTab],
        activeTabId: activeTab.id,
        repositoryMutationError: {
          resource,
          action: "stage",
          kind: "failed",
          message: "Git index is locked",
        },
        error: null,
      });

      render(<Workbench />);

      expect(screen.getByRole("alert")).toHaveTextContent(
        "Source Control could not stage src/file.txt",
      );
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Git index is locked",
      );
    },
  );

  it("does not project a repository mutation error into an active Chat", () => {
    const activeTab = tab("chat");
    useWorkbenchStore.setState({
      tabs: [activeTab],
      activeTabId: activeTab.id,
      repositoryMutationError: {
        resource,
        action: "stage",
        kind: "failed",
        message: "Git index is locked",
      },
      error: null,
    });

    render(<Workbench />);

    expect(screen.queryByRole("alert")).toBeNull();
  });
});
