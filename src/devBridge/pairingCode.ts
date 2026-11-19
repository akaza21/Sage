// The entry imports this module first, so the single-use pairing code leaves
// the address before any other module (including the app graph) evaluates.
let pairingCode: string | null = null;

if (import.meta.env.DEV && new URLSearchParams(location.search).has("bridge")) {
  pairingCode = location.hash.slice(1);
  history.replaceState(history.state, "", location.pathname + location.search);
}

export function takePairingCode(): string {
  const code = pairingCode ?? "";
  pairingCode = null;
  return code;
}
