import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useRef, type ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem, TurnProjection } from "../../domain/acp";
import { clearApprovalDecisions, useApprovalSlot } from "./useApprovalSlot";

afterEach(() => {
  cleanup();
  clearApprovalDecisions();
  vi.useRealTimers();
  document.body.replaceChildren();
});

function permission(id = "permission-item"): AcpItem {
  return {
    id,
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "streaming",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: null,
    kind: "permissionRequest",
    toolState: "waitingForApproval",
    payload: {
      requestId: `request-${id}`,
      state: "received",
      label: "Permission request",
      summary: "Run a command",
      offeredOptionKinds: ["allow_once", "reject_once"],
      category: "execute",
    },
  } as AcpItem;
}

function turn(items: AcpItem[]) {
  return { id: "turn-1", items } as unknown as TurnProjection;
}

function Pane({
  items,
  focused = true,
  extra,
}: {
  items: AcpItem[];
  focused?: boolean;
  extra?: ReactNode;
}) {
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const approval = useApprovalSlot(turn(items), "chat-1", composerRef, {
    focused,
    onCancelTurn: () => undefined,
  });
  return (
    <section className="chat-surface" aria-label="Pane">
      <div className="message-list">
        <article tabIndex={0} aria-label="Answer">
          Answer
        </article>
      </div>
      <div className="chat-find">
        <input aria-label="Search chat" />
      </div>
      {extra}
      {approval.slot}
      <textarea
        ref={composerRef}
        aria-label="Message agent"
        hidden={approval.awaiting}
      />
    </section>
  );
}

const allow = () => screen.getByRole("button", { name: /Allow once/ });
const announced = () =>
  screen.findByText("Approval needed: Allow this command?");

async function raiseWith(focus: () => HTMLElement) {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(100_000);
  const view = render(<Pane items={[]} />);
  const target = focus();
  target.focus();
  vi.setSystemTime(110_000);
  view.rerender(<Pane items={[permission()]} />);
  return target;
}

describe("useApprovalSlot focus", () => {
  it("leaves focus alone while the person is typing in the composer", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(50_000);
    const { rerender } = render(<Pane items={[]} />);
    const composer = screen.getByRole("textbox", { name: "Message agent" });
    composer.focus();
    fireEvent.keyDown(composer, { key: "a" });
    vi.setSystemTime(50_200);
    rerender(<Pane items={[permission()]} />);
    expect(await announced()).toBeInTheDocument();
    expect(allow()).not.toHaveFocus();
  });

  it("does not take focus from a composer that was focused, even without typing", async () => {
    await raiseWith(() =>
      screen.getByRole("textbox", { name: "Message agent" }),
    );
    expect(await announced()).toBeInTheDocument();
    expect(allow()).not.toHaveFocus();
  });

  it("never takes focus from this pane's find bar", async () => {
    const target = await raiseWith(() =>
      screen.getByRole("textbox", { name: "Search chat" }),
    );
    expect(await announced()).toBeInTheDocument();
    expect(target).toHaveFocus();
  });

  it.each(["iframe", "input", "button"])(
    "never takes focus from a %s outside the pane (preview, rename, other pane)",
    async (selector) => {
      const outside = document.createElement("div");
      outside.innerHTML =
        '<iframe title="Preview"></iframe><input aria-label="Rename chat" />' +
        '<section class="chat-surface"><div class="message-list"><button>Other pane</button></div></section>';
      document.body.append(outside);
      const target = await raiseWith(() =>
        outside.querySelector<HTMLElement>(selector)!,
      );
      expect(await announced()).toBeInTheDocument();
      expect(document.activeElement).toBe(target);
    },
  );

  it("takes focus from this pane's own transcript", async () => {
    await raiseWith(() => screen.getByRole("article", { name: "Answer" }));
    await waitFor(() => expect(allow()).toHaveFocus());
  });

  it("takes focus from the body once the last focused element is gone", async () => {
    const gone = document.createElement("input");
    document.body.append(gone);
    gone.focus();
    gone.remove();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(200_000);
    render(<Pane items={[permission("again")]} />);
    await waitFor(() => expect(allow()).toHaveFocus());
  });

  it("never takes focus in a background pane", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(300_000);
    render(<Pane items={[permission("other")]} focused={false} />);
    expect(await announced()).toBeInTheDocument();
    expect(allow()).not.toHaveFocus();
  });
});

describe("useApprovalSlot cards", () => {
  it("gives a harness asking questions the question card, not the approval", async () => {
    const question = permission("question");
    if (question.kind !== "permissionRequest") throw new Error("unreachable");
    question.payload.userInput = {
      questions: [
        {
          id: "scope",
          header: "Scope",
          question: "Which files should I change?",
          allowsOther: false,
          secret: false,
          kind: "text",
          multiSelect: false,
          optional: false,
        },
      ],
    };
    render(<Pane items={[question]} />);
    expect(
      await screen.findByRole("group", { name: "Agent question" }),
    ).toHaveTextContent("Which files should I change?");
    expect(screen.queryByRole("button", { name: /Allow once/ })).toBeNull();
  });
});
