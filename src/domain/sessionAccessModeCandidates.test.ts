import { expect, it } from "vitest";
import {
  sessionAccessEffective,
  sessionAccessModeCandidates,
  type SessionAccessModeCapability,
} from "./sessionAccessMode";
it("preserves native policy labels and descriptions without adding unsupported policies", () => {
  const capabilities = [
    {
      id: "ask" as const,
      label: "Ask each time",
      description: "Review each permission request; no filesystem sandbox.",
    },
  ];
  expect(sessionAccessModeCandidates(capabilities)).toEqual([
    {
      id: "ask",
      label: "Ask each time",
      description: "Review each permission request; no filesystem sandbox.",
    },
  ]);
});
it("does not invent unsupported access modes", () => {
  expect(sessionAccessModeCandidates(undefined)).toEqual([]);
});

it("rejects malformed IPC label and description values", () => {
  expect(
    sessionAccessModeCandidates([
      { id: "ask", label: 42, description: "review" },
      { id: "ask", label: "Ask", description: null },
    ] as never),
  ).toEqual([]);
});

const claude: SessionAccessModeCapability[] = [
  {
    id: "ask",
    label: "Ask each time",
    description: "Review each permission request.",
    fallback: true,
    sessionModeId: "default",
  },
  {
    id: "acceptEdits",
    label: "Accept edits",
    description: "File edits run without asking.",
    sessionModeId: "acceptEdits",
  },
  {
    id: "plan",
    label: "Plan",
    description: "Claude plans without changing files.",
    sessionModeId: "plan",
  },
  {
    id: "fullAccess",
    label: "Full access",
    description: "Everything runs without asking.",
    sessionModeId: "bypassPermissions",
  },
];
const ids = (candidates: readonly SessionAccessModeCapability[]) =>
  candidates.map((candidate) => candidate.id);

it("offers only the modes the live session advertises", () => {
  // Claude run as root advertises no bypass mode, and this one no plan mode.
  const session = {
    sessionId: "session-1",
    availableModeIds: ["default", "acceptEdits", "auto"],
    fullAccess: false,
  };
  expect(ids(sessionAccessModeCandidates(claude, session))).toEqual([
    "ask",
    "acceptEdits",
  ]);
});

it("offers Full access only to a live session, never as a new-chat default", () => {
  expect(ids(sessionAccessModeCandidates(claude))).toEqual([
    "ask",
    "acceptEdits",
    "plan",
  ]);
  expect(
    ids(
      sessionAccessModeCandidates(claude, {
        sessionId: "session-1",
        availableModeIds: null,
        fullAccess: true,
      }),
    ),
  ).toEqual(["ask", "acceptEdits", "plan"]);
});

it("runs a granted session in Full access and every other one at its default", () => {
  const advertised = ["default", "acceptEdits", "plan", "bypassPermissions"];
  const granted = {
    sessionId: "session-1",
    availableModeIds: advertised,
    fullAccess: true,
  };
  const other = { ...granted, sessionId: "session-2", fullAccess: false };
  expect(
    sessionAccessEffective(
      "acceptEdits",
      sessionAccessModeCandidates(claude, granted),
      granted,
    ),
  ).toBe("fullAccess");
  expect(
    sessionAccessEffective(
      "acceptEdits",
      sessionAccessModeCandidates(claude, other),
      other,
    ),
  ).toBe("acceptEdits");
  expect(
    sessionAccessEffective(
      undefined,
      sessionAccessModeCandidates(claude, other),
      other,
    ),
  ).toBe("ask");
});
