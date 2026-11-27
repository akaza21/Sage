import {
  type ModelOption,
  type SimulatorTurnResult,
} from "../domain/workbench";
import { invokeNative } from "./nativeInvoke";
import { type AgentPort, type SendPromptRequest } from "./workbenchPorts";

export const nativeWorkbenchAgentPort: AgentPort = {
  listHarnesses() {
    return invokeNative("list_harnesses");
  },
  refreshHarnesses() {
    return invokeNative("refresh_harnesses");
  },
  listHarnessModels(harnessId, remembered): Promise<ModelOption[]> {
    return invokeNative("list_harness_models", { harnessId, remembered });
  },
  sendPrompt(request: SendPromptRequest): Promise<SimulatorTurnResult> {
    return invokeNative("send_simulated_prompt", { ...request });
  },
};
