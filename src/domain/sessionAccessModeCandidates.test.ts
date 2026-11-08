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
    id: "auto",
    label: "Auto",
    description: "Claude runs what it judges safe.",
    sessionModeId: "auto",
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
  const advertised = ["default", "acceptEdits", "auto"];
  expect(ids(sessionAccessModeCandidates(claude, advertised))).toEqual([
    "ask",
    "acceptEdits",
    "auto",
  ]);
});

it("offers Full access only to a live session, never as a new-chat default", () => {
  expect(ids(sessionAccessModeCandidates(claude))).toEqual([
    "ask",
    "acceptEdits",
    "plan",
    "auto",
  ]);
  expect(ids(sessionAccessModeCandidates(claude, null))).toEqual([
    "ask",
    "acceptEdits",
    "plan",
    "auto",
  ]);
});

it("runs a chat in its own mode and every other chat at the default", () => {
  const candidates = sessionAccessModeCandidates(claude, [
    "default",
    "acceptEdits",
    "plan",
    "bypassPermissions",
  ]);
  expect(sessionAccessEffective("acceptEdits", candidates, "fullAccess")).toBe(
    "fullAccess",
  );
  expect(sessionAccessEffective("acceptEdits", candidates, "plan")).toBe(
    "plan",
  );
  expect(sessionAccessEffective("acceptEdits", candidates, null)).toBe(
    "acceptEdits",
  );
  expect(sessionAccessEffective(undefined, candidates, undefined)).toBe("ask");
  // A choice the session no longer offers falls back to the default.
  expect(sessionAccessEffective("plan", candidates, "auto")).toBe("plan");
});
