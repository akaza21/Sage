import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AcpItem } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { PermissionPrompt } from "./PermissionPrompt";
import { clearApprovalDecisions, resolveApproval } from "../useApprovalSlot";

afterEach(() => {
  cleanup();
  clearApprovalDecisions();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

function permission(
  offeredOptionKinds: readonly string[] | null = [
    "allow_once",
    "allow_always",
    "reject_once",
  ],
  extra: Partial<PermissionProjectionItem["payload"]> = {},
  id = "permission-item",
): PermissionProjectionItem {
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
      summary: 'curl.exe -s -o NUL -w "%{http_code}" https://example.com',
      offeredOptionKinds: offeredOptionKinds ?? undefined,
      category: "execute",
      reason:
        "May I retry the network check outside the sandbox? The sandboxed request returned 000 without an HTTP response.",
      ...extra,
    },
  } as PermissionProjectionItem;
}

function accepted(decision: string) {
  return vi.spyOn(acpDesktop, "resolvePermissionDecision").mockResolvedValue({
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    requestId: "request-permission-item",
    decision,
    state: decision === "deny" ? "declinePending" : "allowPending",
  } as never);
}

describe("PermissionPrompt", () => {
  it("shares the fallback's exact approval identity for every decision (R3-09)", async () => {
    const resolve = accepted("allowAlways");
    const item = permission();
    await resolveApproval(item, "allowAlways");
    expect(resolve).toHaveBeenCalledWith({
      projectId: item.projectId,
      chatId: item.chatId,
      sessionId: item.sessionId,
      turnId: item.turnId,
      runtimeEpoch: item.runtimeEpoch,
      requestId: item.payload.requestId,
      decision: "allowAlways",
    });
  });

  it("marks the safe initial Allow focus and clears it when focus leaves (R3-09)", () => {
    render(<PermissionPrompt items={[permission()]} />);
    const allow = screen.getByRole("button", { name: /Allow once/ });
    expect(allow).toHaveFocus();
    expect(allow).toHaveAttribute("data-initial-focus", "true");
    fireEvent.focus(screen.getByRole("button", { name: /Deny/ }));
    fireEvent.blur(allow);
    expect(allow).not.toHaveAttribute("data-initial-focus");
  });

  it.each([{ focused: false }, { canTakeFocus: () => false }])(
    "does not mark unsafe initial focus (R3-09)",
    (props) => {
      render(<PermissionPrompt items={[permission()]} {...props} />);
      expect(
        screen.getByRole("button", { name: /Allow once/ }),
      ).not.toHaveAttribute("data-initial-focus");
    },
  );
  it("keeps Sage's question as the headline and the agent's reason secondary", () => {
    render(<PermissionPrompt items={[permission()]} />);
    const prompt = screen.getByRole("group", { name: "Permission request" });
    expect(within(prompt).getByText("Terminal")).toBeVisible();
    expect(
      prompt.querySelector(".acp-permission-prompt__question"),
    ).toHaveTextContent("Allow this command?");
    const reason = prompt.querySelector(".acp-permission-prompt__reason")!;
    expect(reason).toHaveTextContent(
      /^Agent's reason:\s*May I retry the network check/,
    );
    expect(prompt.querySelector("pre")).toHaveTextContent(
      'curl.exe -s -o NUL -w "%{http_code}" https://example.com',
    );
  });

  it("shows only the request's own text, never a guessed command from the turn", () => {
    render(
      <PermissionPrompt
        items={[permission(undefined, { summary: "Run a command" })]}
      />,
    );
    expect(document.querySelector("pre")).toHaveTextContent("Run a command");
  });

  it("falls back to a neutral question and no reason line when the harness gave none", () => {
    render(<PermissionPrompt items={[permission(null, { reason: null })]} />);
    expect(screen.getByText("Allow this command?")).toBeVisible();
    expect(document.querySelector(".acp-permission-prompt__reason")).toBeNull();
    render(
      <PermissionPrompt
        items={[
          permission(null, { reason: undefined, category: "other" }, "two"),
        ]}
      />,
    );
    expect(screen.getByText("Allow this action?")).toBeVisible();
  });

  it("keeps a quiet Stop in the header, orders Deny then Allow once, focuses the primary, and offers the menu", () => {
    render(<PermissionPrompt items={[permission()]} onCancelTurn={vi.fn()} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((button) => button.textContent)).toEqual([
      "Stop",
      "Deny Esc",
      "Allow once ⏎",
      "",
    ]);
    // Codex's card has no Stop among its decisions (s6/f0526); Sage keeps
    // one for safety, as a secondary header control.
    const stop = screen.getByRole("button", { name: "Stop turn" });
    expect(stop.closest("header")).not.toBeNull();
    expect(stop.closest(".acp-permission-prompt__actions")).toBeNull();
    expect(screen.getByRole("button", { name: /Allow once/ })).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "More allow options" }));
    const menu = screen.getByRole("menu", { name: "Allow options" });
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Allow once", "Allow similar commands"]);
  });

  it("never fabricates an option the harness did not offer", () => {
    render(
      <PermissionPrompt items={[permission(["allow_once", "reject_once"])]} />,
    );
    expect(
      screen.queryByRole("button", { name: "More allow options" }),
    ).toBeNull();
    cleanup();
    render(<PermissionPrompt items={[permission(["reject_once"])]} />);
    expect(screen.queryByRole("button", { name: /Allow/ })).toBeNull();
    expect(screen.getByRole("button", { name: /Deny/ })).toBeEnabled();
    cleanup();
    render(<PermissionPrompt items={[permission(null)]} />);
    expect(screen.queryByRole("button", { name: /Allow/ })).toBeNull();
    cleanup();
    render(
      <PermissionPrompt
        items={[
          permission(["allow_always", "allow_once"], { category: "edit" }),
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "More allow options" }));
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual(["Always allow", "Allow once"]);
    expect(screen.getByText("Edit")).toBeVisible();
  });

  it("keeps Stop available so a card with no offered choice never strands the pane", () => {
    const cancel = vi.fn();
    render(<PermissionPrompt items={[permission([])]} onCancelTurn={cancel} />);
    expect(screen.queryByRole("button", { name: /Allow/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Stop turn" }));
    expect(cancel).toHaveBeenCalledOnce();
  });

  it("uses Sage's browser choices for a browser permission", () => {
    render(
      <PermissionPrompt
        items={[
          permission(null, {
            source: "browser",
            category: "mcp",
            summary: "Browser fill on https://example.com",
          }),
        ]}
      />,
    );
    expect(
      screen.getByRole("group", { name: "Browser permission" }),
    ).toBeVisible();
    expect(screen.getByText("Browser")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "More allow options" }));
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual(["Allow once", "Allow for this tab and origin"]);
  });

  it("never reroutes Enter to Allow once: Deny, the chevron and the card text keep their own meaning", () => {
    const resolve = accepted("deny");
    render(<PermissionPrompt items={[permission()]} />);
    for (const target of [
      screen.getByRole("button", { name: /Deny/ }),
      screen.getByRole("button", { name: "More allow options" }),
      screen.getByText("Allow this command?"),
      screen.getByRole("group", { name: "Permission request" }),
    ])
      fireEvent.keyDown(target, { key: "Enter" });
    expect(resolve).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /Deny/ }));
    expect(resolve).toHaveBeenLastCalledWith({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      turnId: "turn-1",
      runtimeEpoch: "1",
      requestId: "request-permission-item",
      decision: "deny",
    });
  });

  it("denies on Escape from inside the card, but never on a key that started outside it", () => {
    const resolve = accepted("deny");
    render(<PermissionPrompt items={[permission()]} />);
    const allow = screen.getByRole("button", { name: /Allow once/ });
    // A held key from the composer arrives as repeats or a bare keyup.
    expect(fireEvent.keyDown(allow, { key: "Escape", repeat: true })).toBe(
      false,
    );
    expect(fireEvent.keyDown(allow, { key: "Enter", repeat: true })).toBe(
      false,
    );
    expect(fireEvent.keyUp(allow, { key: " " })).toBe(false);
    expect(resolve).not.toHaveBeenCalled();
    fireEvent.keyDown(allow, { key: " " });
    expect(fireEvent.keyUp(allow, { key: " " })).toBe(true);
    fireEvent.keyDown(screen.getByText(/^May I retry/), { key: "Escape" });
    expect(resolve).toHaveBeenLastCalledWith(
      expect.objectContaining({ decision: "deny" }),
    );
  });

  it("leaves focus in the composer while the person types there and announces politely", async () => {
    const composer = document.createElement("textarea");
    document.body.append(composer);
    composer.focus();
    render(
      <PermissionPrompt items={[permission()]} canTakeFocus={() => false} />,
    );
    expect(composer).toHaveFocus();
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Approval needed: Allow this command?",
    );
    composer.remove();
  });

  it("on its own never takes focus from an editable field, whatever the caller allows", async () => {
    const field = document.createElement("input");
    document.body.append(field);
    field.focus();
    render(<PermissionPrompt items={[permission()]} />);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Approval needed",
    );
    expect(field).toHaveFocus();
    field.remove();
  });

  it("never takes focus in a background pane", async () => {
    render(<PermissionPrompt items={[permission()]} focused={false} />);
    expect(
      screen.getByRole("button", { name: /Allow once/ }),
    ).not.toHaveFocus();
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Approval needed",
    );
  });

  it("does not auto-focus a queued follow-up within 300 ms of the previous decision", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(10_000);
    accepted("allowOnce");
    const first = permission(undefined, {}, "first");
    const second = permission(undefined, { summary: "next" }, "second");
    const third = permission(undefined, { summary: "third" }, "third");
    const { rerender } = render(<PermissionPrompt items={[first, second]} />);
    fireEvent.click(screen.getByRole("button", { name: /Allow once/ }));
    vi.setSystemTime(10_200);
    rerender(<PermissionPrompt items={[second, third]} />);
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Approval needed"),
    );
    expect(
      screen.getByRole("button", { name: /Allow once/ }),
    ).not.toHaveFocus();
    // Focus left the previous request's button for the card itself.
    expect(
      screen.getByRole("group", { name: "Permission request" }),
    ).toHaveFocus();
    vi.setSystemTime(10_600);
    rerender(<PermissionPrompt items={[third]} />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Allow once/ })).toHaveFocus(),
    );
  });

  it("does not deny when Escape closes the open menu", () => {
    const resolve = accepted("allowAlways");
    render(<PermissionPrompt items={[permission()]} />);
    fireEvent.click(screen.getByRole("button", { name: "More allow options" }));
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "Allow once" }), {
      key: "Escape",
    });
    expect(resolve).not.toHaveBeenCalled();
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "More allow options" }));
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Allow similar commands" }),
    );
    expect(resolve).toHaveBeenCalledWith(
      expect.objectContaining({ decision: "allowAlways" }),
    );
  });

  it("disables the actions while the decision is in flight and shows a refusal", async () => {
    const resolve = accepted("allowOnce");
    const { unmount } = render(<PermissionPrompt items={[permission()]} />);
    fireEvent.click(screen.getByRole("button", { name: /Allow once/ }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Allow once/ })).toBeDisabled(),
    );
    fireEvent.keyDown(screen.getByRole("button", { name: /Deny/ }), {
      key: "Escape",
    });
    expect(resolve).toHaveBeenCalledTimes(1);
    unmount();
    resolve.mockRejectedValue(new Error("unsupported approval"));
    // A request already answered stays answered; a new request can fail.
    render(<PermissionPrompt items={[permission(undefined, {}, "refused")]} />);
    fireEvent.click(screen.getByRole("button", { name: /Allow once/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This decision could not be sent",
    );
    expect(screen.getByRole("button", { name: /Deny/ })).toBeEnabled();
  });

  it("shows the oldest of several pending requests with its position", () => {
    render(
      <PermissionPrompt
        items={[
          permission(undefined, {}, "first"),
          permission(undefined, { summary: "later command" }, "second"),
        ]}
      />,
    );
    expect(screen.getByText("1 of 2")).toBeVisible();
    expect(
      screen.getByRole("group", { name: "Permission request" }),
    ).toHaveAttribute("data-acp-item-id", "first");
    expect(screen.queryByText("later command")).toBeNull();
  });
});

describe("PermissionPrompt key repeats", () => {
  it("answers a held key at most once, even when it was pressed inside the card", () => {
    render(<PermissionPrompt items={[permission()]} />);
    const allow = screen.getByRole("button", { name: /Allow once/ });
    expect(fireEvent.keyDown(allow, { key: "Enter" })).toBe(true);
    expect(fireEvent.keyDown(allow, { key: "Enter", repeat: true })).toBe(
      false,
    );
  });
});
