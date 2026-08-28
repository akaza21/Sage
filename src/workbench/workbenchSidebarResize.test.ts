import { describe, expect, it } from "vitest";

import {
  AGENT_ARTIFACT_WIDTH,
  AGENT_LEFT_RAIL_WIDTH,
  EDITOR_TOOL_RAIL_WIDTH,
  resizeWorkbenchSidebar,
} from "./workbenchSidebarResize";

describe("resizeWorkbenchSidebar", () => {
  it("keeps each resizable workbench column within its usable width bounds", () => {
    expect(
      resizeWorkbenchSidebar(AGENT_LEFT_RAIL_WIDTH, { pointerOffset: 90 }),
    ).toBe(180);
    expect(
      resizeWorkbenchSidebar(EDITOR_TOOL_RAIL_WIDTH, { pointerOffset: 314 }),
    ).toBe(314);
    expect(
      resizeWorkbenchSidebar(AGENT_ARTIFACT_WIDTH, { pointerOffset: 800 }),
    ).toBe(460);
  });

  it("uses keyboard steps and Home or End within the same bounds", () => {
    expect(
      resizeWorkbenchSidebar(AGENT_LEFT_RAIL_WIDTH, { key: "ArrowRight" }),
    ).toBe(256);
    expect(
      resizeWorkbenchSidebar(EDITOR_TOOL_RAIL_WIDTH, {
        key: "ArrowLeft",
        shiftKey: true,
      }),
    ).toBe(252);
    expect(resizeWorkbenchSidebar(AGENT_ARTIFACT_WIDTH, { key: "Home" })).toBe(
      280,
    );
    expect(resizeWorkbenchSidebar(AGENT_ARTIFACT_WIDTH, { key: "End" })).toBe(
      460,
    );
  });
});
