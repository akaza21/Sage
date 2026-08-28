import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

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
  clearBrowseSessions,
  setHarnessBrowseOwner,
} from "../features/preview/harnessBrowse";
import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import { AgentArtifactPane } from "./AgentArtifactPane";

const initialState = useWorkbenchStore.getState();

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
  seedFocusedPage();
  setHarnessBrowseOwner("chat-focused");
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
  useAgentArtifactAdmissionStore.getState().clear();
  useWorkbenchStore.setState(initialState, true);
});

describe("Agent artifact file chips", () => {
  it("keeps Preview closed until the focused Chat opens an admitted browse session", () => {
    render(<AgentArtifactPane />);

    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
    expect(
      screen.getByText(
        "Files, diffs, and browser previews from this Chat open here.",
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
      chatId: "chat-focused",
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

  it("shows an Agent-admitted unmatched tab without edit-summary counters", () => {
    useAgentArtifactAdmissionStore.getState().admit("diff:other");
    render(<AgentArtifactPane />);

    const unmatched = screen.getByRole("tab", { name: /^other\.ts$/ });
    expect(within(unmatched).queryByText("+3")).toBeNull();
    expect(within(unmatched).queryByText("-1")).toBeNull();
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
    useAgentArtifactAdmissionStore.getState().admit("file:readme");
    useAgentArtifactAdmissionStore.getState().admit("diff:other");
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
    useAgentArtifactAdmissionStore.getState().admit("file:readme");
    useAgentArtifactAdmissionStore.getState().admit("diff:other");
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
