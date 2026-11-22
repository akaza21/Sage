import { expect, test } from "vitest";
import type {
  AcpCapabilitySet,
  AcpChatPage,
  AcpItem,
  TurnProjection,
} from "../../domain/acp";
import {
  agentSessionFixture,
  capabilitySetFixture,
} from "../../domain/acpProjection.fixture";
import { page as fixture } from "./acp/acpProjection.fixture";
import { conversationActions } from "./conversationActionAvailability";

type Offer = NonNullable<AcpCapabilitySet["rewind"]>;
const all: Offer = {
  effects: { conversation: true, files: false },
  targets: "anyTurn",
  forkAt: true,
  onResume: false,
};

const cases: {
  name: string;
  offer?: Offer;
  expected: boolean[];
  cold?: boolean;
  busy?: boolean;
  compacted?: boolean;
}[] = [
  { name: "missing offer", offer: undefined, expected: [false, false] },
  { name: "conversation and fork", offer: all, expected: [true, true] },
  {
    name: "fork at user message",
    offer: {
      ...all,
      effects: { conversation: false, files: false },
      targets: "anyUserMessage",
    },
    expected: [false, true],
  },
  {
    name: "file effect alone",
    offer: { ...all, effects: { conversation: false, files: true } },
    expected: [false, true],
  },
  {
    name: "conversation alone",
    offer: { ...all, forkAt: false },
    expected: [true, false],
  },
  {
    name: "earlier turn with latest-only offer",
    offer: { ...all, targets: "latestTurn" },
    expected: [false, false],
  },
  { name: "cold session", offer: all, cold: true, expected: [false, false] },
  { name: "busy session", offer: all, busy: true, expected: [false, false] },
  {
    name: "compacted prefix",
    offer: all,
    compacted: true,
    expected: [false, false],
  },
];

test.each(cases)(
  "$name offers the available transcript actions",
  ({ offer, expected, ...options }) => {
    const base = fixture("completed");
    const first = base.turns[0]!;
    const second: TurnProjection = {
      ...first,
      id: "turn-2" as typeof first.id,
      ordinal: 2,
      terminalAt: options.busy ? null : 3,
      items: options.compacted
        ? [
            {
              ...first.items[0],
              kind: "compactionNotice",
              payload: {},
            } as Extract<AcpItem, { kind: "compactionNotice" }>,
          ]
        : [],
    };
    const page: AcpChatPage = {
      ...base,
      turns: [first, second],
      sessions: [
        agentSessionFixture({
          id: first.sessionId,
          residency: options.cold ? "cold" : "hot",
          capabilities: capabilitySetFixture({ rewind: offer }),
        }),
      ],
    };
    const result = conversationActions(page, first);
    expect([result.rewind, result.fork]).toEqual(expected);
    if (options.compacted) {
      const third = { ...first, id: "turn-3" as typeof first.id, ordinal: 3 };
      expect(
        conversationActions(
          { ...page, turns: [first, second, third] } as AcpChatPage,
          third,
        ),
      ).toEqual({ rewind: true, fork: true });
    }
    expect(
      conversationActions({ ...page, turns: [] } as AcpChatPage, first),
    ).toEqual({ rewind: false, fork: false });
  },
);

test.each([undefined, false])(
  "a message without durable identity (%s) has no target actions",
  (conversationTarget) => {
    const base = fixture("completed");
    const turn = { ...base.turns[0]!, conversationTarget };
    const page: AcpChatPage = {
      ...base,
      turns: [turn],
      sessions: [
        agentSessionFixture({
          id: turn.sessionId,
          residency: "hot",
          capabilities: capabilitySetFixture({
            rewind: {
              effects: { conversation: true, files: true },
              targets: "anyTurn",
              forkAt: true,
              onResume: false,
            },
          }),
        }),
      ],
    };
    expect(conversationActions(page, turn)).toEqual({
      rewind: false,
      fork: false,
    });
  },
);

test("latest-turn rewind follows the native undo offer without a message target", () => {
  const base = fixture("completed");
  const first = { ...base.turns[0]!, conversationTarget: false };
  const latest = {
    ...first,
    id: "turn-00000000-0000-4000-8000-000000000099" as typeof first.id,
    ordinal: 2,
  };
  const page: AcpChatPage = {
    ...base,
    turns: [first, latest],
    sessionSurface: { activities: [], undoTurn: latest.id },
    sessions: [
      agentSessionFixture({
        id: latest.sessionId,
        residency: "hot",
        capabilities: capabilitySetFixture({
          rewind: { ...all, targets: "latestTurn", runtimeCheckpoint: true },
        }),
      }),
    ],
  };
  expect(conversationActions(page, latest)).toEqual({
    rewind: true,
    fork: false,
  });
  expect(conversationActions(page, first)).toEqual({
    rewind: false,
    fork: false,
  });
  expect(
    conversationActions({ ...page, sessionSurface: null }, latest),
  ).toEqual({ rewind: false, fork: false });
});
