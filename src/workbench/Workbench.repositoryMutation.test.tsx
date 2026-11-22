import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { settleLazyModules } from "../test/settleLazyModules";

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () => <div>Chat surface</div>,
}));
vi.mock("../features/diff/DiffSurface", () => ({
  DiffSurface: () => <div>Diff surface</div>,
}));
vi.mock("../features/files/FileSurface", () => ({
  default: () => <div>File surface</div>,
}));
vi.mock("./DirtyTabDialog", () => ({ default: () => null }));
vi.mock("./LeftSidebar", () => ({ LeftSidebar: () => null }));
vi.mock("./RightSidebar", () => ({ RightSidebar: () => null }));
vi.mock("./StatusBar", () => ({ StatusBar: () => null }));
vi.mock("./TabStrip", () => ({ default: () => null }));
vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => false,
}));

import { Workbench } from "./Workbench";
import { SourceControlView } from "./SourceControlView";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";

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

beforeEach(() => writeWorkbenchLayout({ windowMode: "editor" }));

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

describe("Workbench repository mutation errors", () => {
  it("owns the mutation explanation once in Source Control with no editor overlay", async () => {
    const activeTab = tab("file");
    useWorkbenchStore.setState({
      tabs: [activeTab],
      activeTabId: activeTab.id,
      repositoryMutationError: {
        resource,
        action: "stage",
        kind: "failed",
        message: "Git index is locked",
      },
      loadRepositoryInventory: async () => undefined,
    });
    const { container } = render(
      <>
        <Workbench />
        <aside aria-label="Source Control test scope">
          <SourceControlView />
        </aside>
      </>,
    );
    await settleLazyModules();
    const sidebar = within(
      screen.getByRole("complementary", { name: "Source Control test scope" }),
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(sidebar.getByRole("alert")).toHaveTextContent(
      "Source Control could not stage src/file.txt.",
    );
    fireEvent.click(sidebar.getByRole("button", { name: "Details" }));
    expect(screen.getAllByText("Git index is locked")).toHaveLength(1);
    expect(container.querySelector(".active-canvas [role=alert]")).toBeNull();
  });
  it.each(["file", "diff"] as const)(
    "links the active %s to the Source Control issue without an editor alert",
    async (kind) => {
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
      await settleLazyModules();

      expect(screen.queryByRole("alert")).toBeNull();
      expect(screen.queryByText("Git index is locked")).toBeNull();
      fireEvent.click(
        screen.getByRole("button", { name: "Show in Source Control" }),
      );
      expect(useWorkbenchStore.getState()).toMatchObject({
        sidebarView: "sourceControl",
        sidebarOpen: true,
      });
      // With Source Control showing, the editor no longer points at it.
      expect(
        screen.queryByRole("button", { name: "Show in Source Control" }),
      ).toBeNull();
    },
  );

  it("does not project a repository mutation error into an active Chat", async () => {
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
    await settleLazyModules();

    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("links containment failures without repeating their explanation in the editor", async () => {
    const activeTab = tab("file");
    useWorkbenchStore.setState({
      tabs: [activeTab],
      activeTabId: activeTab.id,
      repositoryMutationError: {
        resource,
        action: "stage",
        kind: "containment",
        message: "Git termination is proven, but reap did not converge",
      },
      error: null,
    });

    render(<Workbench />);
    await settleLazyModules();

    expect(
      screen.getByRole("button", { name: "Show in Source Control" }),
    ).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      screen.queryByText(
        "Git termination is proven, but reap did not converge",
      ),
    ).toBeNull();
  });
});
