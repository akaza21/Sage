// Q-07 compatibility entrypoint: the Playwright browser lane dynamically
// imports this path only for the common scenario-install route below.
// Everything else (types, catalogue, builders, installers) lives in its
// authoritative module and is imported from there directly.
export { installVisualScenario } from "./visualScenarioShell";
