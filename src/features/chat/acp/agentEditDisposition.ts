import type {
  AcpMutationResult,
  AgentEditDisposition,
} from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";

export async function recordAgentEditDisposition(
  disposition: AgentEditDisposition,
): Promise<AcpMutationResult> {
  return acpDesktop.recordAgentEditDisposition(disposition);
}
