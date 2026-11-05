type Acp04Ui = Readonly<{
  codexReady: string;
  createFreshChat: string;
  seededChatId: string;
}>;

interface Acp04Selection {
  id: string;
  text: string;
}

interface Acp04Snapshot {
  model: Acp04Selection;
  [field: string]: unknown;
}

interface Acp04ModeAState {
  readonly phase: "await-first" | "await-second" | "complete";
  readonly model: Acp04Selection;
  readonly [field: string]: unknown;
}

export const ACP04_SNAPSHOT_EXPRESSION: string;
export function createAcp04Ui(codexVersion: string): Acp04Ui;
export function freshChatReadyExpression(previousChatId: string): string;
export function createModeAState(
  echoes: readonly string[],
  initialValue: unknown,
  codexVersion: string,
): Acp04ModeAState;
export function acceptModeASnapshot(
  state: Acp04ModeAState,
  value: unknown,
): Acp04ModeAState;
export type { Acp04Snapshot };
