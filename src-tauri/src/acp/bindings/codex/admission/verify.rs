use std::{fs::File, io::Read};

use sha2::{Digest, Sha256};

use super::{
    AdmittedCodexBinding, BINDING_DIGEST, BINDING_DIGEST_INPUT, BINDING_ID,
    CodexAdmissionUnavailable, EntrypointIdentity, MetadataValidatedPacket, observed_environment,
};

impl AdmittedCodexBinding {
    pub(crate) fn revalidate(&self) -> Result<(), CodexAdmissionUnavailable> {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if let Some(fixture) = &self.desktop_fixture {
            return fixture
                .selection
                .revalidate()
                .map_err(|_| CodexAdmissionUnavailable::Drift);
        }
        if let Some(observed) = &self.observed {
            let current = observed_entrypoint_identity(&self.entrypoint.path)?;
            return (current.sha256 == self.entrypoint.sha256
                && current.byte_len == self.entrypoint.byte_len
                && observed.environment
                    == observed_environment(&self.entrypoint.path, observed.inputs.as_ref())?)
            .then_some(())
            .ok_or(CodexAdmissionUnavailable::Drift);
        }
        self.reproduced
            .as_ref()
            .ok_or(CodexAdmissionUnavailable::Drift)?
            .revalidate(&self.packet)
            .map_err(|_| CodexAdmissionUnavailable::Drift)?;
        self.authority
            .as_ref()
            .ok_or(CodexAdmissionUnavailable::Drift)?
            .revalidate(
                &self.packet,
                self.reproduced
                    .as_ref()
                    .ok_or(CodexAdmissionUnavailable::Drift)?,
            )
            .map_err(|_| CodexAdmissionUnavailable::Drift)?;
        self.environment
            .as_ref()
            .ok_or(CodexAdmissionUnavailable::Drift)?
            .revalidate(&self.packet)
            .map_err(|_| CodexAdmissionUnavailable::Drift)?;
        let current = entrypoint_identity(&self.packet)?;
        if current.path != self.entrypoint.path
            || current.sha256 != self.entrypoint.sha256
            || current.byte_len != self.entrypoint.byte_len
            || self.packet.metadata().binding_id != BINDING_ID
            || <[u8; 32]>::from(Sha256::digest(BINDING_DIGEST_INPUT)) != BINDING_DIGEST
        {
            return Err(CodexAdmissionUnavailable::Drift);
        }
        Ok(())
    }
}

pub(super) fn observed_entrypoint_identity(
    path: &std::path::Path,
) -> Result<EntrypointIdentity, CodexAdmissionUnavailable> {
    const MAX_OBSERVED_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
    let path = path
        .canonicalize()
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let metadata = path
        .metadata()
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_OBSERVED_EXECUTABLE_BYTES
    {
        return Err(CodexAdmissionUnavailable::IndependentAdmission);
    }
    let mut file =
        File::open(&path).map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    Ok(EntrypointIdentity {
        path,
        sha256: digest.finalize().into(),
        byte_len: metadata.len(),
    })
}

pub(super) fn observed_binding_digest(entrypoint: &EntrypointIdentity) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"sage:observed-codex-app-server:v1:");
    digest.update(entrypoint.path.as_os_str().as_encoded_bytes());
    digest.update(entrypoint.sha256);
    digest.update(entrypoint.byte_len.to_le_bytes());
    digest.finalize().into()
}

pub(super) fn sha256_hex(digest: [u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn entrypoint_identity(
    packet: &MetadataValidatedPacket,
) -> Result<EntrypointIdentity, CodexAdmissionUnavailable> {
    let entrypoint = &packet.metadata().entrypoint;
    Ok(EntrypointIdentity {
        path: entrypoint.path.clone(),
        sha256: decode_sha256(&entrypoint.sha256)?,
        byte_len: entrypoint.byte_len,
    })
}

fn decode_sha256(value: &str) -> Result<[u8; 32], CodexAdmissionUnavailable> {
    if value.len() != 64 {
        return Err(CodexAdmissionUnavailable::Drift);
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(digest)
}

fn hex_nibble(value: u8) -> Result<u8, CodexAdmissionUnavailable> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(CodexAdmissionUnavailable::Drift),
    }
}
