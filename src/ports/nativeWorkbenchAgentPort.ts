import type { AgentPort, SendPromptRequest } from "./workbenchPorts";
import type { ModelOption, SimulatorTurnResult } from "../domain/workbench";
import { invokeNative } from "./nativeInvoke";

export const nativeWorkbenchAgentPort: AgentPort = {
  listHarnesses() {
    return invokeNative("list_harnesses");
  },
  listHarnessModels(harnessId: string): Promise<ModelOption[]> {
    return invokeNative("list_harness_models", { harnessId });
  },
  sendPrompt(request: SendPromptRequest): Promise<SimulatorTurnResult> {
    return invokeNative("send_simulated_prompt", { ...request });
  },
};
