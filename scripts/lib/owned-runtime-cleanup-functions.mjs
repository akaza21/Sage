// The functions a test must load from run-source-functional-native-ide-journey.ps1
// to run its owned-runtime cleanup: Stop-OwnedRuntimeTree and everything its
// failure paths call. A test that loads only part of this set reaches an
// undefined command on a failure path, and Windows PowerShell then searches
// every module on PSModulePath for it, which can take longer than the test's
// time limit on a hosted runner.
export const OWNED_RUNTIME_CLEANUP_FUNCTIONS = Object.freeze([
  "Get-CleanupRootTuple",
  "Resolve-CleanupFailureClass",
  "Test-StartupIncompleteIdentityFailure",
  "Get-StartupIncompleteIdentityProcessId",
  "Test-StartupIncompleteIdentityFieldPresent",
  "Add-StartupIncompleteIdentityFact",
  "Update-OwnedRuntimeTree",
  "Write-RuntimeCleanupQuarantine",
  "Stop-OwnedRuntimeTree",
  "Publish-PendingJourneyReceipt",
]);
