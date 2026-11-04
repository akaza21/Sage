import { invokeNative } from "../ports/nativeInvoke";

/** Terminal preferences load with the workbench, outside startup bindings. */
export const getTerminalDockVisible = () =>
  invokeNative<boolean>("get_terminal_dock_visible");
export const setTerminalDockVisible = (visible: boolean) =>
  invokeNative<boolean>("set_terminal_dock_visible", { visible });
