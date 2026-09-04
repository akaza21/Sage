import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import type {
  AcpChatPage,
  AcpItem,
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../domain/acp";
import {
  applyBrowseIntent,
  clearBrowseSession,
  clearBrowseSessions,
} from "../features/preview/harnessBrowse";
import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "../app/agentArtifactFollowStore";
import { clearAgentChatOwner, pinAgentChatOwner } from "../app/agentChatOwner";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { AgentArtifactPane } from "./AgentArtifactPane";

const initialState = useWorkbenchStore.getState();
const focusedOwner = {
  projectId: "project-a",
  chatId: "chat-focused",
  sessionId: "session-a",
} as AgentArtifactOwner;

function admit(tabId: string): void {
  useAgentArtifactAdmissionStore.getState().admit({
    owner: focusedOwner,
    tabId,
  });
}

function pageWithEdit(): AcpChatPage {
  const item = {
    id: "item-a",
    projectId: "project-a",
    chatId: "chat-focused",
    sessionId: "session-a",
    turnId: "turn-a",
    runtimeEpoch: "3",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
    kind: "fileChangeDiff",
    toolState: null,
    payload: {
      changes: [
        {
          resource: {
            kind: "resource",
            resource: { rootId: "root-a", relativePath: "src/app.ts" },
          },
          operation: "updated",
          originalResource: null,
          summary: "edited",
          additions: 3,
          deletions: 1,
        },
      ],
      truncated: false,
    },
  } as unknown as AcpItem;
  return {
    projectId: "project-a",
    chatId: "chat-focused",
    title: "Focused",
    projectionRevision: "8",
    currentSessionId: "session-a",
    sessions: [{ id: "session-a", ordinal: 1 }],
    turns: [
      {
        id: "turn-a",
        sessionId: "session-a",
        runtimeEpoch: "3",
        ordinal: 2,
        state: "completed",
        revision: "1",
        createdAt: 1,
        updatedAt: 2,
        terminalAt: 2,
        items: [item],
        activities: [],
      },
    ],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

function seedFocusedPage(): void {
  pinAcpProjectionIds(["chat-focused"]);
  const store = acpProjectionStoreFor("chat-focused");
  const scope = {
    projectId: "project-a" as SageProjectId,
    projectGeneration: "0" as ProjectGeneration,
    chatId: "chat-focused" as SageChatId,
  };
  store.getState().activateScope(scope);
  expect(store.getState().applyAuthoritativePage(scope, pageWithEdit())).toBe(
    "accepted",
  );
}

beforeEach(() => {
  useAgentArtifactFollowStore.getState().clear();
  seedFocusedPage();
  pinAgentChatOwner(focusedOwner.chatId);
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "Project A", roots: [] },
    projectGeneration: 0,
    projectTransitioning: false,
    tabs: [
      {
        id: "chat-focused",
        kind: "chat",
        title: "Focused",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "file:app",
        kind: "file",
        title: "app.ts",
        position: 1,
        pinned: false,
        dirty: false,
        resource: { rootId: "root-a", relativePath: "src/app.ts" },
      },
      {
        id: "diff:other",
        kind: "diff",
        title: "other.ts",
        position: 2,
        pinned: false,
        dirty: false,
        resource: { rootId: "root-a", relativePath: "src/other.ts" },
      },
    ],
    activeTabId: "chat-focused",
  });
});

afterEach(() => {
  cleanup();
  clearBrowseSessions();
  clearAcpProjectionRegistry();
  clearAgentChatOwner();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  useWorkbenchStore.setState(initialState, true);
});

describe("Agent artifact file chips", () => {
  it("starts empty as a collapsed right-edge resource affordance", () => {
    const onExpandedChange = vi.fn();
    pinAgentChatOwner(null);
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-focused",
    });

    const { rerender } = render(
      <AgentArtifactPane
        expanded={false}
        onExpandedChange={onExpandedChange}
      />,
    );

    const expand = screen.getByRole("button", {
      name: "Open files, changes, and preview",
    });
    expect(expand).toBeVisible();
    expect(within(expand).queryByText("Files")).toBeNull();
    expect(within(expand).queryByText("Changes")).toBeNull();
    expect(within(expand).queryByText("Preview")).toBeNull();
    expect(
      screen.queryByRole("heading", { name: "No resources yet" }),
    ).toBeNull();

    fireEvent.click(expand);
    expect(onExpandedChange).toHaveBeenLastCalledWith(true);

    rerender(
      <AgentArtifactPane expanded onExpandedChange={onExpandedChange} />,
    );
    expect(
      screen.getByRole("heading", { name: "No resources yet" }),
    ).toBeVisible();
    expect(
      screen.getByText(
        "Files, changes, and previews from this Chat will appear here.",
      ),
    ).toBeVisible();
    fireEvent.keyDown(screen.getByLabelText("Agent artifacts"), {
      key: "Escape",
    });
    expect(onExpandedChange).toHaveBeenLastCalledWith(false);

    rerender(
      <AgentArtifactPane
        expanded={false}
        onExpandedChange={onExpandedChange}
      />,
    );
    expect(
      screen.getByRole("button", {
        name: "Open files, changes, and preview",
      }),
    ).toHaveFocus();
  });

  it("requests expansion when a new Agent file becomes available", () => {
    const onExpandedChange = vi.fn();
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-focused",
    });
    render(
      <AgentArtifactPane
        expanded={false}
        onExpandedChange={onExpandedChange}
      />,
    );

    act(() => {
      useWorkbenchStore.setState((state) => ({
        tabs: [
          ...state.tabs,
          {
            id: "file:new",
            kind: "file",
            title: "new.ts",
            position: 1,
            pinned: false,
            dirty: false,
            resource: { rootId: "root-a", relativePath: "src/new.ts" },
          },
        ],
      }));
      admit("file:new");
    });

    expect(onExpandedChange).toHaveBeenCalledWith(true);
  });

  it("keeps Preview closed until the focused Chat opens an admitted browse session", () => {
    render(<AgentArtifactPane />);

    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
    expect(
      screen.getByText(
        "Files, changes, and previews from this Chat will appear here.",
      ),
    ).toBeInTheDocument();
  });

  it("shows +3 −1 on a matched file tab and hides an Editor-only unmatched tab", () => {
    render(<AgentArtifactPane />);

    const matched = screen.getByRole("tab", { name: /app\.ts/ });
    expect(within(matched).getByText("+3")).toBeInTheDocument();
    expect(within(matched).getByText("-1")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /^other\.ts$/ })).toBeNull();

    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
  });

  it("opens Agent Preview as a loopback browser without the app-script starter", () => {
    applyBrowseIntent({
      owner: focusedOwner,
      url: "http://127.0.0.1:5173/",
    });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    expect(screen.getByLabelText("Sage browser")).toBeInTheDocument();
    expect(screen.getByLabelText("Preview address")).toBeInTheDocument();
    expect(
      screen.queryByText("No dev or start scripts were found."),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
  });

  it("keeps a pinned File when Preview arrives and resumes Follow latest", () => {
    useAgentArtifactFollowStore
      .getState()
      .pin(focusedOwner, { kind: "tab", tabId: "file:app" });
    render(<AgentArtifactPane />);

    expect(screen.getByRole("tab", { name: /app\.ts/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    act(() => {
      applyBrowseIntent({
        owner: focusedOwner,
        url: "http://127.0.0.1:5173/preview",
      });
    });

    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    const follow = screen.getByRole("button", {
      name: "Follow latest resource",
    });
    expect(follow).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(follow);
    expect(follow).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    act(() => clearBrowseSession(focusedOwner));
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
    expect(screen.getByRole("tab", { name: /app\.ts/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("pins an explicitly selected resource tab for the focused Chat", () => {
    admit("diff:other");
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: /^other\.ts$/ }));

    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        focusedOwner,
      ),
    ).toMatchObject({
      mode: "pinned",
      selected: { kind: "tab", tabId: "diff:other" },
    });
  });

  it("shows an Agent-admitted unmatched tab without edit-summary counters", () => {
    admit("diff:other");
    render(<AgentArtifactPane />);

    const unmatched = screen.getByRole("tab", { name: /^other\.ts$/ });
    expect(within(unmatched).queryByText("+3")).toBeNull();
    expect(within(unmatched).queryByText("-1")).toBeNull();
  });

  it("hides an admission during Project transition and after session replacement", () => {
    admit("diff:other");
    render(<AgentArtifactPane />);
    expect(screen.getByRole("tab", { name: /^other\.ts$/ })).toBeVisible();

    act(() => useWorkbenchStore.setState({ projectTransitioning: true }));
    expect(screen.queryByRole("tab", { name: /^other\.ts$/ })).toBeNull();

    act(() =>
      useWorkbenchStore.setState({
        projectTransitioning: false,
        projectGeneration: 1,
      }),
    );
    expect(screen.queryByRole("tab", { name: /^other\.ts$/ })).toBeNull();

    act(() => {
      useWorkbenchStore.setState({ projectGeneration: 0 });
      const replacementOwner = {
        ...focusedOwner,
        sessionId: "session-new",
      } as AgentArtifactOwner;
      acpProjectionStoreFor(focusedOwner.chatId).setState({
        page: {
          ...pageWithEdit(),
          currentSessionId: replacementOwner.sessionId,
        },
      });
    });

    expect(screen.queryByRole("tab", { name: /^other\.ts$/ })).toBeNull();
  });

  it("uses language-aware file icons on file tabs and keeps the diff glyph", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "file:app",
          kind: "file",
          title: "app.ts",
          position: 1,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/app.ts" },
        },
        {
          id: "file:readme",
          kind: "file",
          title: "README.md",
          position: 2,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "README.md" },
        },
        {
          id: "diff:other",
          kind: "diff",
          title: "other.ts",
          position: 3,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/other.ts" },
        },
      ],
      activeTabId: "chat-focused",
    });
    admit("file:readme");
    admit("diff:other");
    render(<AgentArtifactPane />);

    expect(
      screen
        .getByRole("tab", { name: /app\.ts/ })
        .querySelector(".file-kind-icon--typescript"),
    ).not.toBeNull();
    expect(
      screen
        .getByRole("tab", { name: /README\.md/ })
        .querySelector(".file-kind-icon--markdown"),
    ).not.toBeNull();
    expect(
      screen
        .getByRole("tab", { name: /^other\.ts$/ })
        .querySelector(".file-kind-icon"),
    ).toBeNull();
  });

  it("shows +3 −1 on a matched diff tab", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "diff:app",
          kind: "diff",
          title: "app.ts",
          position: 1,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/app.ts" },
        },
      ],
      activeTabId: "chat-focused",
    });
    render(<AgentArtifactPane />);

    const matched = screen.getByRole("tab", { name: /app\.ts/ });
    expect(within(matched).getByText("+3")).toBeInTheDocument();
    expect(within(matched).getByText("-1")).toBeInTheDocument();
  });

  it("shows a dirty dot only on a dirty file tab", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "file:app",
          kind: "file",
          title: "app.ts",
          position: 1,
          pinned: false,
          dirty: true,
          resource: { rootId: "root-a", relativePath: "src/app.ts" },
        },
        {
          id: "file:readme",
          kind: "file",
          title: "README.md",
          position: 2,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "README.md" },
        },
        {
          id: "diff:other",
          kind: "diff",
          title: "other.ts",
          position: 3,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/other.ts" },
        },
      ],
      activeTabId: "file:app",
    });
    admit("file:readme");
    admit("diff:other");
    render(<AgentArtifactPane />);

    expect(
      within(screen.getByRole("tab", { name: /app\.ts/ })).getByTitle(
        "Unsaved changes",
      ),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("tab", { name: /README\.md/ })).queryByTitle(
        "Unsaved changes",
      ),
    ).toBeNull();
    expect(
      within(screen.getByRole("tab", { name: /^other\.ts$/ })).queryByTitle(
        "Unsaved changes",
      ),
    ).toBeNull();
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
  });

  it("closes an Agent artifact without adding a close control to Preview", () => {
    render(<AgentArtifactPane />);

    fireEvent.click(screen.getByRole("button", { name: "Close app.ts" }));
    expect(screen.queryByRole("tab", { name: /app\.ts/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Close Preview" })).toBeNull();
  });
});
