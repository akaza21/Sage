import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AnswerUserInputRequest } from "../domain/acp";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { acpDesktop } from "./acpDesktop";
import { safeItem } from "./acpDesktopResponseItems";

function questionItem(userInput: unknown) {
  return safeItem({
    id: "item-question",
    projectId: "project-one",
    chatId: "chat-one",
    sessionId: "session-one",
    turnId: "turn-one",
    activityId: null,
    runtimeEpoch: "1",
    revision: "1",
    ordinal: 1,
    origin: "primary",
    kind: "permissionRequest",
    lifecycle: "streaming",
    toolState: "waitingForApproval",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: null,
    payload: {
      requestId: "request-one",
      state: "received",
      label: "Permission request",
      summary: "Answer the agent's questions",
      offeredOptionKinds: ["reject_once"],
      category: "other",
      userInput,
    },
  });
}

const answer: AnswerUserInputRequest = {
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
  turnId: "turn-a",
  runtimeEpoch: "2",
  requestId: "request-a",
  answers: [{ questionId: "scope", selectedOption: "All" }],
};

beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  invokeMock.mockReset();
});

describe("harness question requests", () => {
  it("decode the questions and clamp the auto-resolution window", () => {
    const item = questionItem({
      questions: [
        {
          id: "scope",
          header: "Scope",
          question: "Which files?",
          options: [{ label: "All", description: "Every file" }],
          allowsOther: true,
          secret: false,
        },
        {
          id: "token",
          header: "Token",
          question: "API token?",
          allowsOther: false,
          secret: true,
        },
      ],
      autoResolutionMs: 90_000_000,
    });
    expect(
      item?.kind === "permissionRequest" && item.payload.userInput,
    ).toEqual({
      questions: [
        {
          id: "scope",
          header: "Scope",
          question: "Which files?",
          options: [{ label: "All", description: "Every file" }],
          allowsOther: true,
          secret: false,
        },
        {
          id: "token",
          header: "Token",
          question: "API token?",
          allowsOther: false,
          secret: true,
        },
      ],
      autoResolutionMs: 86_400_000,
    });
  });

  it("refuse an item whose questions break the native caps", () => {
    const question = { id: "q", header: "", question: "?" };
    for (const userInput of [
      { questions: [] },
      { questions: Array(5).fill(question) },
      {
        questions: [
          {
            ...question,
            options: Array(9).fill({ label: "a", description: "" }),
          },
        ],
      },
      { questions: [{ ...question, id: "" }] },
      { questions: [question], autoResolutionMs: -1 },
    ]) {
      expect(questionItem(userInput)).toBeNull();
    }
    expect(questionItem(undefined)?.kind).toBe("permissionRequest");
  });

  it("answer through the native command and accept only the matching reply", async () => {
    const reply = {
      ...answer,
      answers: undefined,
      decision: "answer",
      state: "allowPending",
    };
    invokeMock.mockResolvedValueOnce(reply);
    await expect(acpDesktop.answerUserInput(answer)).resolves.toEqual({
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
      turnId: "turn-a",
      runtimeEpoch: "2",
      requestId: "request-a",
      decision: "answer",
      state: "allowPending",
    });
    expect(invokeMock).toHaveBeenCalledWith("answer_user_input", {
      request: answer,
    });

    invokeMock.mockResolvedValueOnce({ ...reply, requestId: "request-b" });
    await expect(acpDesktop.answerUserInput(answer)).rejects.toMatchObject({
      failure: { kind: "internal" },
    });
  });
});
