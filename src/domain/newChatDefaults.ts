/**
 * Model and effort belong to the harness they were picked in, so the native
 * store replaces the record whole.
 */
export interface NewChatDefaults {
  harnessId: string | null;
  modelId: string | null;
  effort: string | null;
}

export type DefaultHarnessStatus =
  "available" | "detecting" | "unavailable" | "notInstalled" | "unknown";

export type DefaultOptionStatus = "available" | "unavailable" | "unknown";

export interface NewChatDefaultsState {
  defaults: NewChatDefaults;
  harnessStatus: DefaultHarnessStatus | null;
  modelStatus: DefaultOptionStatus | null;
  effortStatus: DefaultOptionStatus | null;
}
