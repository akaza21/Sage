import { useState } from "react";
import { pinnedAcpProjectionIds } from "../../app/acpProjectionRegistry";
import AgentChatRuntime from "../../workbench/AgentChatRuntime";

export default function AgentSettingsExecution() {
  const [ids] = useState(pinnedAcpProjectionIds);
  return <AgentChatRuntime ids={ids} visible={false} />;
}
