mod beta_evidence;
mod chat;
mod editor;
mod file_operations;
mod harness;
mod project;
mod repository;
mod resource;

#[allow(unused_imports)]
pub use beta_evidence::{
    BetaEvidenceCandidateIdentity, BetaEvidenceDescriptor, BetaEvidenceResult,
};
pub use chat::*;
pub use editor::*;
pub use file_operations::*;
pub use harness::*;
pub use project::*;
pub use repository::*;
pub use resource::*;

#[cfg(test)]
mod freeze_tests {
    use super::BetaEvidenceResult;

    #[test]
    fn domain_reexports_beta_evidence_result() {
        let result = BetaEvidenceResult::incomplete("contract", "commit");
        assert_eq!(result.verdict, "incomplete");
    }
}
