import { useEffect, useState } from "react";
import { invokeNative, isTauriRuntime } from "../../ports/nativeInvoke";
import type { AvailableAcpCommand } from "../../services/acpCommandsDesktop";

/** As `MAX_DRAFT_BUILTINS` in the native command placement. */
const MAX_BUILTINS = 32;

/**
 * Insert-only rows for the draft's "Skills and commands" section: the
 * harness's curated built-in commands from installation metadata
 * (`list_builtin_commands`). None outside the desktop app or on failure.
 */
export function useDraftBuiltinCommands(harnessId: string | undefined) {
  const [rows, setRows] = useState<AvailableAcpCommand[]>([]);
  useEffect(() => {
    let live = !!harnessId && isTauriRuntime();
    if (live)
      invokeNative<unknown>("list_builtin_commands", { harnessId }).then(
        (names) =>
          live &&
          Array.isArray(names) &&
          setRows(
            names.slice(0, MAX_BUILTINS).map((name) => ({
              name: String(name),
              description: "Built-in",
              inputHint: null,
              source: "builtin",
              placement: { section: "skillsAndCommands" },
            })),
          ),
        () => undefined,
      );
    return () => {
      live = false;
    };
  }, [harnessId]);
  return rows;
}
