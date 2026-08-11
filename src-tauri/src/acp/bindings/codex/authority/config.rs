use sha2::{Digest, Sha256};

use crate::acp::bindings::codex::policy::SecurityPolicyPrerequisite;

const CONFIG_BYTES_MAX: usize = 4 * 1024;
const PINNED_CONFIG: &[u8] = include_bytes!("../../../../../testdata/acp/codex/config-v1.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ConfigAuthorityError {
    #[error("Codex configuration exceeds its byte limit")]
    ConfigBytes,
    #[error("Codex configuration is not the exact pinned configuration")]
    Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerifiedCodexConfig {
    digest: [u8; 32],
}

impl VerifiedCodexConfig {
    pub(super) fn revalidate(
        &self,
        policy: &SecurityPolicyPrerequisite,
    ) -> Result<(), ConfigAuthorityError> {
        let expected = decode_digest(policy.config_sha256())?;
        if self.digest != expected || Sha256::digest(PINNED_CONFIG).as_slice() != expected {
            return Err(ConfigAuthorityError::Config);
        }
        Ok(())
    }
}

pub(crate) fn verify_codex_config(
    bytes: &[u8],
    policy: &SecurityPolicyPrerequisite,
) -> Result<VerifiedCodexConfig, ConfigAuthorityError> {
    if bytes.len() > CONFIG_BYTES_MAX {
        return Err(ConfigAuthorityError::ConfigBytes);
    }
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let proof = VerifiedCodexConfig { digest };
    proof.revalidate(policy)?;
    if bytes != PINNED_CONFIG {
        return Err(ConfigAuthorityError::Config);
    }
    Ok(proof)
}

fn decode_digest(value: &str) -> Result<[u8; 32], ConfigAuthorityError> {
    if value.len() != 64 {
        return Err(ConfigAuthorityError::Config);
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Ok(digest)
}

fn nibble(value: u8) -> Result<u8, ConfigAuthorityError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(ConfigAuthorityError::Config),
    }
}
