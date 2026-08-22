#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BetaEvidenceCandidateIdentity {
    pub platform: String,
    pub architecture: String,
    pub source_commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BetaEvidenceDescriptor {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BetaEvidenceResult {
    pub schema_version: u32,
    pub work_item: String,
    pub contract_sha256: String,
    pub candidate: BetaEvidenceCandidateIdentity,
    pub evidence: Vec<BetaEvidenceDescriptor>,
    pub verdict: String,
}

impl BetaEvidenceResult {
    pub fn incomplete(
        contract_sha256: impl Into<String>,
        source_commit: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            work_item: "BETA-0001".into(),
            contract_sha256: contract_sha256.into(),
            candidate: BetaEvidenceCandidateIdentity {
                platform: "windows".into(),
                architecture: "x86_64".into(),
                source_commit: source_commit.into(),
            },
            evidence: Vec::new(),
            verdict: "incomplete".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BetaEvidenceResult;

    #[test]
    fn incomplete_result_freezes_beta_identity() {
        let result = BetaEvidenceResult::incomplete("abc", "def");
        assert_eq!(result.schema_version, 1);
        assert_eq!(result.work_item, "BETA-0001");
        assert_eq!(result.verdict, "incomplete");
        assert_eq!(result.candidate.platform, "windows");
        assert!(result.evidence.is_empty());
    }
}
