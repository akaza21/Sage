import type { AcpChatPage } from "../../../../src/domain/acp";

import { standardPage } from "../visualChatPages";
import type { VisualScenarioName } from "../visualScenarioTypes";

export function buildRecoveryPages(
  name: VisualScenarioName,
): AcpChatPage[] | null {
  switch (name) {
    case "error":
      return [
        standardPage("visual-chat-error", "Recover a failed preview", "error"),
      ];
    case "waiting":
      return [
        standardPage("visual-chat-waiting", "Inspecting workspace", "waiting"),
      ];
    default:
      return null;
  }
}
