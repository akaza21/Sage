export type BetaEvidenceVerdict = "passed" | "failed" | "incomplete";

export interface BetaEvidenceCandidateIdentity {
  platform: "windows";
  architecture: "x86_64";
  sourceCommit: string;
}

export interface BetaEvidenceDescriptor {
  id: string;
  status: "passed" | "failed" | "missing";
}

export interface BetaEvidenceResult {
  schemaVersion: 1;
  workItem: "BETA-0001";
  contractSha256: string;
  candidate: BetaEvidenceCandidateIdentity;
  evidence: readonly BetaEvidenceDescriptor[];
  verdict: BetaEvidenceVerdict;
}
