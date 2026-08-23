import type { AgentEditDisposition } from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";

export async function recordAgentEditDisposition(
  disposition: AgentEditDisposition,
): Promise<void> {
  await acpDesktop.recordAgentEditDisposition(disposition);
}
