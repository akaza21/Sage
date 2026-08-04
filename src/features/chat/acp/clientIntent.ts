import type { ClientIntentId } from "../../../domain/acp";
import { AcpDesktopError } from "../../../services/acpDesktop";

const MAX_INTENT_SEQUENCE = 1_000_000;

export function createClientIntentFactory(): () => ClientIntentId {
  let sequence = 0;
  const cryptoId = globalThis.crypto?.randomUUID?.();
  const bootId =
    cryptoId ??
    `${Date.now().toString(36)}-${Math.floor(
      globalThis.performance?.timeOrigin ?? 0,
    ).toString(36)}`;
  return () => {
    if (sequence >= MAX_INTENT_SEQUENCE) {
      throw new AcpDesktopError({
        kind: "capacity",
        resource: "queue",
        limit: MAX_INTENT_SEQUENCE,
        message: "The ACP client intent limit was reached; restart Sage.",
      });
    }
    sequence += 1;
    return `intent-ui-${sequence.toString(36).padStart(4, "0")}-${bootId}` as ClientIntentId;
  };
}
