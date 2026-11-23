import type { AcpSessionMutationIntent } from "../domain/acp";
import { invokeAcp } from "./acpDesktopCommand";

// Only the lazy slash-menu footer needs the refresh port.
export const acpCommandCatalogDesktop = {
  async refreshCommands(request: AcpSessionMutationIntent): Promise<void> {
    await invokeAcp("refresh_acp_commands", request);
  },
};
