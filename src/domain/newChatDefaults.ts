/**
 * The defaults a new chat starts from. Model and effort belong to the harness
 * they were picked in, so the native store replaces the record whole; a
 * record without a harness clears it.
 */
export interface NewChatDefaults {
  harnessId: string | null;
  modelId: string | null;
  effort: string | null;
}

/**
 * What harness discovery says about the stored default harness. A default
 * that cannot run is reported, never rewritten, and a new chat then starts
 * the way it would without a default.
 */
export type DefaultHarnessStatus =
  "available" | "detecting" | "unavailable" | "notInstalled" | "unknown";

export interface NewChatDefaultsState {
  defaults: NewChatDefaults;
  /** `null` when no default harness is stored. */
  harnessStatus: DefaultHarnessStatus | null;
}
