/**
 * Model and effort belong to the harness they were picked in, so the native
 * store replaces the record whole; one without a harness clears it.
 */
export interface NewChatDefaults {
  harnessId: string | null;
  modelId: string | null;
  effort: string | null;
}

/** A default that cannot run is reported, never rewritten. */
export type DefaultHarnessStatus =
  "available" | "detecting" | "unavailable" | "notInstalled" | "unknown";

/** `unknown`: nothing discovered can judge it yet, so a new chat checks it
 * against the session's configuration. */
export type DefaultOptionStatus = "available" | "unavailable" | "unknown";

/** Each status is null when that value is not stored. */
export interface NewChatDefaultsState {
  defaults: NewChatDefaults;
  harnessStatus: DefaultHarnessStatus | null;
  modelStatus: DefaultOptionStatus | null;
  effortStatus: DefaultOptionStatus | null;
}
