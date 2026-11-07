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

/**
 * What discovery says about a stored default model or effort. `unknown`
 * means nothing discovered can judge it yet (the harness is not available,
 * or it offers the option only through a live session), so a new chat checks
 * it against the session's configuration.
 */
export type DefaultOptionStatus = "available" | "unavailable" | "unknown";

export interface NewChatDefaultsState {
  defaults: NewChatDefaults;
  /** Each status is `null` when that value is not stored. */
  harnessStatus: DefaultHarnessStatus | null;
  modelStatus: DefaultOptionStatus | null;
  effortStatus: DefaultOptionStatus | null;
}
