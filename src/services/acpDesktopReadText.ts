import { invokeNative } from "../ports/nativeInvoke";
import type {
  AcpReadTextFileFlushAck,
  AcpReadTextNextAction,
} from "./acpReadTextFileFlush";
import { AcpDesktopError, safeFailure } from "./acpDesktop";

/** Typed, no-content terminal acknowledgement for the deferred P-02 seam. */
export async function acknowledgeReadTextFileFlush(
  ack: AcpReadTextFileFlushAck,
): Promise<AcpReadTextNextAction> {
  try {
    return await invokeNative<AcpReadTextNextAction>(
      "acknowledge_acp_read_text_file",
      { ack },
    );
  } catch (error: unknown) {
    throw new AcpDesktopError(safeFailure(error));
  }
}
