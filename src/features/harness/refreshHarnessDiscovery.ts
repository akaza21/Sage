import { useWorkbenchStore } from "../../app/store";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";

/**
 * Asks the native side to discover harnesses again and applies the result,
 * unless a newer inventory (from discovery or another refresh) arrived while
 * this one ran. Settings and the composer's harness menu share it.
 */
export async function refreshHarnessDiscovery(): Promise<void> {
  const initialInventory = useWorkbenchStore.getState().harnesses;
  const inventory = await getWorkbenchPorts().agent.refreshHarnesses();
  const current = useWorkbenchStore.getState();
  if (current.harnesses === initialInventory)
    await current.applyHarnessDiscovery(inventory);
}
