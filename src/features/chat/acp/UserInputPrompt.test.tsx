import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ComponentProps } from "react";

import type { AcpItem, UserInputRequest } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { clearApprovalDecisions } from "../useApprovalSlot";
import { UserInputPrompt } from "./UserInputPrompt";

type PermissionProjectionItem = Extract<AcpItem, { kind: "permissionRequest" }>;

afterEach(() => {
  cleanup();
  clearApprovalDecisions();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const identity = {
  projectId: "project-1",
  chatId: "chat-1",
  sessionId: "session-1",
  turnId: "turn-1",
  runtimeEpoch: "1",
  requestId: "request-1",
};

function questionRequest(
  userInput: UserInputRequest,
  createdAt = 1,
): PermissionProjectionItem {
  return {
    id: "question-item",
    projectId: identity.projectId,
    chatId: identity.chatId,
    sessionId: identity.sessionId,
    turnId: identity.turnId,
    runtimeEpoch: identity.runtimeEpoch,
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "streaming",
    revision: "1",
    createdAt,
    updatedAt: createdAt,
    terminalAt: null,
    kind: "permissionRequest",
    toolState: "waitingForApproval",
    payload: {
      requestId: identity.requestId,
      state: "received",
      label: "Question",
      summary: "Answer the agent's questions",
      offeredOptionKinds: ["allow_once", "reject_once"],
      reason: null,
      userInput,
    },
  } as PermissionProjectionItem;
}

const questions: UserInputRequest = {
  questions: [
    {
      id: "scope",
      header: "Scope",
      question: "Which files should I change?",
      options: [
        { label: "Only tests", description: "Leave production code alone." },
        { label: "Everything", description: "" },
      ],
      allowsOther: true,
      secret: false,
      kind: "text",
      multiSelect: false,
      optional: false,
    },
    {
      id: "token",
      header: "Token",
      question: "Paste the deploy token.",
      allowsOther: false,
      secret: true,
      kind: "text",
      multiSelect: false,
      optional: false,
    },
  ],
};

const submit = () => screen.getByRole("button", { name: /Submit/ });

/** The card as the approval slot mounts it, for the oldest request. */
function QuestionCard({
  items,
  ...props
}: Omit<ComponentProps<typeof UserInputPrompt>, "item" | "userInput">) {
  const [item] = items;
  return (
    <UserInputPrompt
      {...props}
      items={items}
      item={item!}
      userInput={item!.payload.userInput!}
    />
  );
}

const field = (
  id: string,
  header: string,
  extra: Partial<UserInputRequest["questions"][number]>,
) => ({
  id,
  header,
  question: "",
  allowsOther: false,
  secret: false,
  kind: "text" as const,
  multiSelect: false,
  optional: false,
  ...extra,
});

describe("UserInputPrompt forms and links", () => {
  const answered = () =>
    vi.spyOn(acpDesktop, "answerUserInput").mockResolvedValue({
      ...identity,
      decision: "answer",
      state: "allowPending",
    });

  it("answers checkboxes, yes/no, numbers and an editor, skipping optional fields", () => {
    const answer = answered();
    const options = (...labels: string[]) =>
      labels.map((label) => ({ label, description: "" }));
    render(
      <QuestionCard
        items={[
          questionRequest({
            questions: [
              field("checks", "Checks", {
                options: options("Lint", "Tests"),
                multiSelect: true,
              }),
              field("agree", "Agree", {
                options: options("Yes", "No"),
                kind: "boolean",
              }),
              field("count", "Count", { kind: "integer" }),
              field("notes", "Notes", {
                kind: "multiline",
                initialText: "Line 1\nLine 2",
              }),
              field("nick", "Nickname", { optional: true }),
            ],
          }),
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "Lint" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Tests" }));
    fireEvent.click(screen.getByRole("radio", { name: "Yes" }));
    const count = screen.getByLabelText("Answer: Count");
    fireEvent.input(count, { target: { value: "2.5" } });
    expect(count).toHaveAccessibleDescription("Enter a number.");
    expect(submit()).toBeDisabled();
    fireEvent.input(count, { target: { value: "3" } });
    const notes = screen.getByLabelText("Answer: Notes");
    expect(notes.tagName).toBe("TEXTAREA");
    expect(submit()).toBeEnabled();

    fireEvent.click(submit());
    expect(answer).toHaveBeenCalledWith({
      ...identity,
      answers: [
        { questionId: "checks", selectedOptions: ["Lint", "Tests"] },
        { questionId: "agree", selectedOption: "Yes" },
        { questionId: "count", text: "3" },
        { questionId: "notes", text: "Line 1\nLine 2" },
      ],
    });
  });

  it("shows where a link leads and opens it only on the user's word", () => {
    const answer = answered();
    const deny = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        ...identity,
        decision: "deny",
        state: "declinePending",
      });
    render(
      <QuestionCard
        items={[
          questionRequest({
            questions: [],
            url: {
              url: "https://auth.example.com/authorize?state=1",
              host: "auth.example.com",
            },
          }),
        ]}
      />,
    );
    const card = screen.getByRole("group", { name: "Agent link" });
    expect(card).toHaveTextContent("auth.example.com");
    expect(card).toHaveTextContent(
      "https://auth.example.com/authorize?state=1",
    );
    fireEvent.keyDown(card, { key: "Escape" });
    expect(deny).toHaveBeenCalledTimes(1);
    expect(answer).not.toHaveBeenCalled();
  });

  it("opens the link with an empty answer", () => {
    const answer = answered();
    render(
      <QuestionCard
        items={[
          questionRequest({
            questions: [],
            url: { url: "https://auth.example.com/", host: "auth.example.com" },
          }),
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Open link/ }));
    expect(answer).toHaveBeenCalledWith({ ...identity, answers: [] });
  });
});

describe("UserInputPrompt", () => {
  it("shows the questions instead of an approval and sends the chosen answers", async () => {
    const answer = vi.spyOn(acpDesktop, "answerUserInput").mockResolvedValue({
      ...identity,
      decision: "answer",
      state: "allowPending",
    });
    const deny = vi.spyOn(acpDesktop, "resolvePermissionDecision");
    render(<QuestionCard items={[questionRequest(questions)]} />);

    const card = screen.getByRole("group", { name: "Agent question" });
    expect(card).toHaveTextContent("Which files should I change?");
    expect(card).toHaveTextContent("Leave production code alone.");
    expect(card).toHaveTextContent("Paste the deploy token.");
    expect(card).not.toHaveTextContent("Allow");
    expect(submit()).toBeDisabled();

    fireEvent.click(screen.getByRole("radio", { name: /Only tests/ }));
    expect(submit()).toBeDisabled();
    const secret = screen.getByLabelText("Answer: Token");
    expect(secret).toHaveAttribute("type", "password");
    fireEvent.input(secret, { target: { value: "s3cret-value" } });
    expect(submit()).toBeEnabled();
    expect(card.textContent).not.toContain("s3cret-value");
    expect(card.innerHTML).not.toContain("s3cret-value");

    fireEvent.click(submit());
    expect(answer).toHaveBeenCalledWith({
      ...identity,
      answers: [
        { questionId: "scope", selectedOption: "Only tests" },
        { questionId: "token", text: "s3cret-value" },
      ],
    });
    expect(deny).not.toHaveBeenCalled();
    expect(submit()).toBeDisabled();
  });

  it("answers with free text when Other is chosen, and only once it holds text", () => {
    const answer = vi.spyOn(acpDesktop, "answerUserInput").mockResolvedValue({
      ...identity,
      decision: "answer",
      state: "allowPending",
    });
    render(
      <QuestionCard
        items={[questionRequest({ questions: [questions.questions[0]!] })]}
      />,
    );
    fireEvent.click(screen.getByRole("radio", { name: "Other" }));
    const other = screen.getByLabelText("Other answer: Scope");
    fireEvent.input(other, { target: { value: "   " } });
    expect(submit()).toBeDisabled();
    fireEvent.input(other, { target: { value: "Only docs" } });
    fireEvent.click(submit());
    expect(answer).toHaveBeenCalledWith({
      ...identity,
      answers: [{ questionId: "scope", text: "Only docs" }],
    });
  });

  it("declines through the ordinary decline path, by button or Esc", () => {
    const deny = vi
      .spyOn(acpDesktop, "resolvePermissionDecision")
      .mockResolvedValue({
        ...identity,
        decision: "deny",
        state: "declinePending",
      } as never);
    const answer = vi.spyOn(acpDesktop, "answerUserInput");
    render(<QuestionCard items={[questionRequest(questions)]} />);
    fireEvent.keyDown(screen.getByRole("radio", { name: /Only tests/ }), {
      key: "Escape",
    });
    expect(deny).toHaveBeenCalledTimes(1);
    expect(deny).toHaveBeenCalledWith({ ...identity, decision: "deny" });
    // The decision is in flight: a second press sends nothing more.
    fireEvent.click(screen.getByRole("button", { name: /Decline/ }));
    expect(deny).toHaveBeenCalledTimes(1);
    expect(answer).not.toHaveBeenCalled();
  });

  it("counts down an auto-resolution window with one timer, cleared on unmount", () => {
    vi.useFakeTimers({ now: 100_000 });
    const { unmount } = render(
      <QuestionCard
        items={[
          questionRequest({ ...questions, autoResolutionMs: 65_000 }, 100_000),
        ]}
      />,
    );
    expect(screen.getByText("Closes unanswered in 1:05")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(60_000));
    expect(screen.getByText("Closes unanswered in 0:05")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(5_000));
    expect(screen.getByText("Closing unanswered…")).toBeInTheDocument();
    expect(vi.getTimerCount()).toBe(0);
    unmount();
  });

  it("clears its timer when the card unmounts early", () => {
    vi.useFakeTimers({ now: 50_000 });
    const { unmount } = render(
      <QuestionCard
        items={[
          questionRequest({ ...questions, autoResolutionMs: 30_000 }, 50_000),
        ]}
      />,
    );
    expect(screen.getByText("Closes unanswered in 0:30")).toBeInTheDocument();
    unmount();
    // Short-lived timeouts settle; an uncleared interval would remain.
    vi.advanceTimersByTime(1_000);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps a chosen option when focus only passes through the Other field", () => {
    render(
      <QuestionCard
        items={[questionRequest({ questions: [questions.questions[0]!] })]}
      />,
    );
    const option = screen.getByRole("radio", { name: /Everything/ });
    fireEvent.click(option);
    const other = screen.getByLabelText("Other answer: Scope");
    fireEvent.focus(other);
    fireEvent.blur(other);
    expect(option).toBeChecked();
    expect(submit()).toBeEnabled();
    fireEvent.input(other, { target: { value: "Only docs" } });
    expect(screen.getByRole("radio", { name: "Other" })).toBeChecked();
  });

  it("explains a free-text answer over the byte limit and keeps Submit off", () => {
    render(
      <QuestionCard
        items={[questionRequest({ questions: [questions.questions[0]!] })]}
      />,
    );
    const other = screen.getByLabelText("Other answer: Scope");
    // 6,000 CJK characters are 18,000 UTF-8 bytes.
    fireEvent.input(other, { target: { value: "字".repeat(6_000) } });
    expect(other).toHaveAttribute("aria-invalid", "true");
    expect(other).toHaveAccessibleDescription(
      "Too long to send: answers are limited to 16,384 bytes.",
    );
    expect(submit()).toBeDisabled();
    fireEvent.input(other, { target: { value: "字".repeat(1_000) } });
    expect(screen.queryByText(/Too long to send/)).toBeNull();
    expect(submit()).toBeEnabled();
  });

  it("lets a key held inside the card repeat in its fields, but not one held from outside", () => {
    render(<QuestionCard items={[questionRequest(questions)]} />);
    const field = screen.getByLabelText("Answer: Token");
    // A repeat whose keydown came before the card is swallowed.
    expect(fireEvent.keyDown(field, { key: "ArrowLeft", repeat: true })).toBe(
      false,
    );
    expect(fireEvent.keyDown(field, { key: "Backspace" })).toBe(true);
    expect(fireEvent.keyDown(field, { key: "Backspace", repeat: true })).toBe(
      true,
    );
  });
});
