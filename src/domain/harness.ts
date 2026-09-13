export type HarnessInstallState =
  "available" | "notInstalled" | "installing" | "unavailable";

interface HarnessDefinitionBase {
  id: string;
  name: string;
  description: string;
  source: "builtIn" | "registry" | "custom" | "development";
  accountModelDiscovery: boolean;
  /** Bounded numeric display version, never raw CLI output or a native path. */
  version?: string | null;
}

export type HarnessDefinition = HarnessDefinitionBase &
  (
    | {
        transport: "legacySimulator";
        installState: HarnessInstallState;
        bindingId: null;
      }
    | {
        transport: "acp";
        installState: "available";
        bindingId: string;
      }
    | {
        transport: "acp";
        installState: Exclude<HarnessInstallState, "available">;
        bindingId: string | null;
      }
  );

export interface ModelOption {
  id: string;
  name: string;
  description?: string;
  isDefault?: boolean;
}
