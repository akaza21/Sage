import { exactKeys, SHA256 } from "./ade0006a-c0b-common.mjs";

const idPattern = /^[a-z0-9][a-z0-9-]{0,63}$/u;

export function validatePackageCleanup(cleanup, errors, index, frozen) {
  const label = `retentionLedger.cleanups[${index}]`;
  exactKeys(
    errors,
    cleanup,
    [
      "cleanupId",
      "allocationIdentity",
      "completedAtUtc",
      "removedCampaigns",
      "removedFiles",
      "removedBytes",
    ],
    label,
  );
  if (
    !idPattern.test(cleanup?.cleanupId ?? "") ||
    !SHA256.test(cleanup?.allocationIdentity ?? "") ||
    Number.isNaN(Date.parse(cleanup?.completedAtUtc ?? "")) ||
    !Array.isArray(cleanup?.removedCampaigns) ||
    cleanup.removedCampaigns.length < 1 ||
    cleanup.removedCampaigns.length >
      frozen.evidence.packageCampaignRetention.ledgerEntries ||
    !Number.isSafeInteger(cleanup?.removedFiles) ||
    cleanup.removedFiles < 0 ||
    !Number.isSafeInteger(cleanup?.removedBytes) ||
    cleanup.removedBytes < 0
  ) {
    errors.push(`${label} identity or totals are invalid`);
    return;
  }
  let files = 0;
  let bytes = 0;
  for (const [removedIndex, removed] of cleanup.removedCampaigns.entries()) {
    const removedLabel = `${label}.removedCampaigns[${removedIndex}]`;
    exactKeys(
      errors,
      removed,
      ["campaignId", "rawFiles", "rawBytes", "curatedReceipt"],
      removedLabel,
    );
    if (
      !idPattern.test(removed?.campaignId ?? "") ||
      !Number.isSafeInteger(removed?.rawFiles) ||
      removed.rawFiles < 0 ||
      !Number.isSafeInteger(removed?.rawBytes) ||
      removed.rawBytes < 0
    ) {
      errors.push(`${removedLabel} raw accounting is invalid`);
    }
    if (removed?.curatedReceipt !== null) {
      exactKeys(
        errors,
        removed.curatedReceipt,
        ["relativePath", "sha256", "bytes"],
        `${removedLabel}.curatedReceipt`,
      );
      if (
        removed.curatedReceipt?.relativePath !==
          `docs/work-items/ADE-0006A-acp-session-foundation/evidence/package/${removed.campaignId}.json` ||
        !SHA256.test(removed.curatedReceipt?.sha256 ?? "") ||
        !Number.isSafeInteger(removed.curatedReceipt?.bytes) ||
        removed.curatedReceipt.bytes < 1
      ) {
        errors.push(`${removedLabel} curated receipt is invalid`);
      }
    }
    files += removed.rawFiles + (removed.curatedReceipt ? 1 : 0);
    bytes += removed.rawBytes + (removed.curatedReceipt?.bytes ?? 0);
  }
  if (files !== cleanup.removedFiles || bytes !== cleanup.removedBytes) {
    errors.push(`${label} totals do not reconcile`);
  }
}
