import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import type { AcpItem } from "../../../src/domain/acp";
import { MermaidFigure } from "../../../src/features/chat/acp/MermaidFigure";
import { PermissionItem } from "../../../src/features/chat/acp/PermissionItem";
import "../../../src/styles/global.css";
import "../../../src/features/chat/acp/acp-chat.css";
import "../../../src/features/chat/acp/acp-chat-activities.css";

const root = createRoot(document.getElementById("root")!);
const item = {
  id: "permission-item",
  kind: "permissionRequest",
  toolState: "waitingForApproval",
  payload: {
    state: "received",
    label: "Write requested",
    summary: "Review edit",
  },
} as Extract<AcpItem, { kind: "permissionRequest" }>;

Object.assign(window, {
  renderRichAnswer(source: string, offeredOptionKinds: string[]) {
    flushSync(() =>
      root.render(
        <div className="agent-chat-pane acp-chat-projection">
          <div className="acp-markdown">
            <MermaidFigure source={source} />
          </div>
          <PermissionItem
            item={{ ...item, payload: { ...item.payload, offeredOptionKinds } }}
          />
        </div>,
      ),
    );
  },
});
