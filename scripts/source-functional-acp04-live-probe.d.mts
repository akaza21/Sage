type Acp04Ui = Readonly<{
  codexReady: string;
  createFreshChat: string;
  seededChatId: string;
}>;

export function createAcp04Ui(codexVersion: string): Acp04Ui;
export function freshChatReadyExpression(previousChatId: string): string;
