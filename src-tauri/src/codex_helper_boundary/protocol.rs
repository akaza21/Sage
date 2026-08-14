use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const REQUEST_PAYLOAD_BYTES_MAX: usize = 128 * 1024;
pub const HELPER_EXECUTABLE_BYTES_MAX: u64 = 32 * 1024 * 1024;
pub const WIRE_REQUEST_BYTES_MAX: usize = 32 + REQUEST_FRAME_BYTES_MAX;
pub const RESPONSE_FRAME_BYTES: usize = RESPONSE_PREFIX_BYTES + AUTHENTICATOR_BYTES;

const REQUEST_MAGIC: &[u8; 8] = b"SGCHREQ1";
const RESPONSE_MAGIC: &[u8; 8] = b"SGCHRSP1";
const SCHEMA_VERSION: u8 = 1;
const AUTHENTICATOR_BYTES: usize = 32;
const REQUEST_PREFIX_BYTES: usize = 8 + 1 + 4 + 32 + 16 + 32 + 8 + 4;
const REQUEST_FRAME_BYTES_MAX: usize =
    REQUEST_PREFIX_BYTES + REQUEST_PAYLOAD_BYTES_MAX + AUTHENTICATOR_BYTES;
const RESPONSE_PREFIX_BYTES: usize = 8 + 1 + 1 + 16 + 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BoundaryError {
    #[error("the helper boundary frame is malformed")]
    Malformed,
    #[error("the helper boundary frame exceeds its byte limit")]
    Bound,
    #[error("the helper boundary authenticator does not match")]
    Authentication,
    #[error("the helper boundary response is not bound to this request")]
    Replay,
    #[error("the helper boundary executable identity does not match")]
    HelperIdentity,
    #[error("the helper boundary process authority does not match")]
    Authority,
    #[error("the helper boundary did not converge before its deadline")]
    Deadline,
    #[error("the helper boundary I/O failed")]
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BoundaryStatus {
    ElevationRequired = 1,
    Published = 3,
    PublicationFailed = 4,
    RecoveryRequired = 5,
}

impl BoundaryStatus {
    fn decode(value: u8) -> Result<Self, BoundaryError> {
        match value {
            1 => Ok(Self::ElevationRequired),
            3 => Ok(Self::Published),
            4 => Ok(Self::PublicationFailed),
            5 => Ok(Self::RecoveryRequired),
            _ => Err(BoundaryError::Malformed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelperIdentity {
    pub sha256: [u8; 32],
    pub byte_len: u64,
}

pub struct OneShotSecret([u8; 32]);

impl OneShotSecret {
    pub(crate) fn generate() -> Self {
        let mut bytes = [0_u8; 32];
        bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
        bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
        Self(bytes)
    }

    #[cfg(test)]
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn from_slice(bytes: &[u8]) -> Result<Self, BoundaryError> {
        if bytes.len() != 32 {
            return Err(BoundaryError::Malformed);
        }
        let mut secret = Self([0; 32]);
        secret.0.copy_from_slice(bytes);
        Ok(secret)
    }

    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[cfg(test)]
    fn fixture(value: u8) -> Self {
        Self([value; 32])
    }
}

impl std::fmt::Debug for OneShotSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OneShotSecret([redacted])")
    }
}

impl Drop for OneShotSecret {
    fn drop(&mut self) {
        clear_volatile(&mut self.0);
    }
}

pub(crate) struct SealedRequest {
    bytes: Vec<u8>,
    digest: [u8; 32],
    nonce: [u8; 16],
}

impl SealedRequest {
    pub(crate) fn seal(
        caller_pid: u32,
        caller_sid_sha256: [u8; 32],
        nonce: [u8; 16],
        helper: HelperIdentity,
        payload: &[u8],
        secret: &OneShotSecret,
    ) -> Result<Self, BoundaryError> {
        if caller_pid == 0
            || helper.byte_len == 0
            || helper.byte_len > HELPER_EXECUTABLE_BYTES_MAX
            || payload.is_empty()
            || payload.len() > REQUEST_PAYLOAD_BYTES_MAX
        {
            return Err(BoundaryError::Bound);
        }
        let mut bytes = Vec::with_capacity(REQUEST_PREFIX_BYTES + payload.len() + 32);
        bytes.extend_from_slice(REQUEST_MAGIC);
        bytes.push(SCHEMA_VERSION);
        bytes.extend_from_slice(&caller_pid.to_le_bytes());
        bytes.extend_from_slice(&caller_sid_sha256);
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&helper.sha256);
        bytes.extend_from_slice(&helper.byte_len.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&hmac_sha256(secret.bytes(), &bytes));
        let digest = Sha256::digest(&bytes).into();
        Ok(Self {
            bytes,
            digest,
            nonce,
        })
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

pub(crate) struct AuthenticatedRequest<'a> {
    pub(crate) caller_pid: u32,
    pub(crate) caller_sid_sha256: [u8; 32],
    pub(crate) nonce: [u8; 16],
    pub(crate) helper: HelperIdentity,
    pub(crate) payload: &'a [u8],
    pub(crate) digest: [u8; 32],
}

pub(crate) fn decode_request<'a>(
    bytes: &'a [u8],
    secret: &OneShotSecret,
) -> Result<AuthenticatedRequest<'a>, BoundaryError> {
    if bytes.len() < REQUEST_PREFIX_BYTES + AUTHENTICATOR_BYTES
        || bytes.len() > REQUEST_FRAME_BYTES_MAX
    {
        return Err(BoundaryError::Bound);
    }
    if &bytes[..8] != REQUEST_MAGIC || bytes[8] != SCHEMA_VERSION {
        return Err(BoundaryError::Malformed);
    }
    let payload_len = u32::from_le_bytes(copy_array(&bytes[101..105])?) as usize;
    let expected_len = REQUEST_PREFIX_BYTES
        .checked_add(payload_len)
        .and_then(|value| value.checked_add(AUTHENTICATOR_BYTES))
        .ok_or(BoundaryError::Bound)?;
    if payload_len == 0 || payload_len > REQUEST_PAYLOAD_BYTES_MAX || bytes.len() != expected_len {
        return Err(BoundaryError::Bound);
    }
    let authenticated_len = bytes.len() - AUTHENTICATOR_BYTES;
    let expected = hmac_sha256(secret.bytes(), &bytes[..authenticated_len]);
    if !constant_time_equal(&expected, &bytes[authenticated_len..]) {
        return Err(BoundaryError::Authentication);
    }
    let caller_pid = u32::from_le_bytes(copy_array(&bytes[9..13])?);
    let helper = HelperIdentity {
        sha256: copy_array(&bytes[61..93])?,
        byte_len: u64::from_le_bytes(copy_array(&bytes[93..101])?),
    };
    if caller_pid == 0 || helper.byte_len == 0 || helper.byte_len > HELPER_EXECUTABLE_BYTES_MAX {
        return Err(BoundaryError::Malformed);
    }
    Ok(AuthenticatedRequest {
        caller_pid,
        caller_sid_sha256: copy_array(&bytes[13..45])?,
        nonce: copy_array(&bytes[45..61])?,
        helper,
        payload: &bytes[REQUEST_PREFIX_BYTES..authenticated_len],
        digest: Sha256::digest(bytes).into(),
    })
}

pub(crate) struct ZeroingWireBuffer<'a> {
    bytes: &'a mut Vec<u8>,
}

impl ZeroingWireBuffer<'_> {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.bytes
    }
}

impl Drop for ZeroingWireBuffer<'_> {
    fn drop(&mut self) {
        clear_volatile(self.bytes);
    }
}

pub(crate) fn wire_request<'a>(
    storage: &'a mut Vec<u8>,
    secret: &OneShotSecret,
    request: &SealedRequest,
) -> ZeroingWireBuffer<'a> {
    clear_volatile(storage);
    storage.clear();
    storage.reserve(32 + request.bytes.len());
    storage.extend_from_slice(secret.bytes());
    storage.extend_from_slice(request.bytes());
    ZeroingWireBuffer { bytes: storage }
}

pub(crate) fn split_wire_request(
    bytes: &mut Vec<u8>,
) -> Result<(OneShotSecret, Vec<u8>), BoundaryError> {
    if bytes.len() <= 32 || bytes.len() > WIRE_REQUEST_BYTES_MAX {
        let secret_prefix_len = bytes.len().min(32);
        clear_volatile(&mut bytes[..secret_prefix_len]);
        return Err(BoundaryError::Bound);
    }
    let secret = OneShotSecret::from_slice(&bytes[..32])?;
    clear_volatile(&mut bytes[..32]);
    let sealed = bytes.split_off(32);
    Ok((secret, sealed))
}

pub(crate) fn seal_response(
    status: BoundaryStatus,
    request: &AuthenticatedRequest<'_>,
    secret: &OneShotSecret,
) -> [u8; RESPONSE_FRAME_BYTES] {
    let mut bytes = [0_u8; RESPONSE_FRAME_BYTES];
    bytes[..8].copy_from_slice(RESPONSE_MAGIC);
    bytes[8] = SCHEMA_VERSION;
    bytes[9] = status as u8;
    bytes[10..26].copy_from_slice(&request.nonce);
    bytes[26..58].copy_from_slice(&request.digest);
    let authenticator = hmac_sha256(secret.bytes(), &bytes[..RESPONSE_PREFIX_BYTES]);
    bytes[RESPONSE_PREFIX_BYTES..].copy_from_slice(&authenticator);
    bytes
}

pub(crate) fn verify_response(
    bytes: &[u8],
    request: &SealedRequest,
    secret: &OneShotSecret,
) -> Result<BoundaryStatus, BoundaryError> {
    if bytes.len() != RESPONSE_FRAME_BYTES {
        return Err(BoundaryError::Bound);
    }
    if &bytes[..8] != RESPONSE_MAGIC || bytes[8] != SCHEMA_VERSION {
        return Err(BoundaryError::Malformed);
    }
    let expected = hmac_sha256(secret.bytes(), &bytes[..RESPONSE_PREFIX_BYTES]);
    if !constant_time_equal(&expected, &bytes[RESPONSE_PREFIX_BYTES..]) {
        return Err(BoundaryError::Authentication);
    }
    if bytes[10..26] != request.nonce || bytes[26..58] != request.digest {
        return Err(BoundaryError::Replay);
    }
    BoundaryStatus::decode(bytes[9])
}

fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut inner_key = [0x36_u8; 64];
    let mut outer_key = [0x5c_u8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_key[index] ^= byte;
        outer_key[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_key);
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(outer_key);
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

fn copy_array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], BoundaryError> {
    bytes.try_into().map_err(|_| BoundaryError::Malformed)
}

fn clear_volatile(bytes: &mut [u8]) {
    for byte in bytes {
        // Best-effort zeroing without widening the dependency graph.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn helper() -> HelperIdentity {
        HelperIdentity {
            sha256: [0x22; 32],
            byte_len: 1_048_576,
        }
    }

    fn request(payload: &[u8], secret: &OneShotSecret) -> SealedRequest {
        SealedRequest::seal(41, [0x11; 32], [0x44; 16], helper(), payload, secret).unwrap()
    }

    #[test]
    fn exact_request_payload_is_accepted_and_bound() {
        let secret = OneShotSecret::fixture(0x33);
        let request = request(&vec![0x55; REQUEST_PAYLOAD_BYTES_MAX], &secret);
        let decoded = decode_request(request.bytes(), &secret).unwrap();

        assert_eq!(decoded.caller_pid, 41);
        assert_eq!(decoded.payload.len(), REQUEST_PAYLOAD_BYTES_MAX);
        assert_eq!(decoded.helper, helper());
        let digest: [u8; 32] = Sha256::digest(request.bytes()).into();
        assert_eq!(request.digest, digest);
    }

    #[test]
    fn request_rejects_plus_one_truncation_trailing_and_tamper() {
        let secret = OneShotSecret::fixture(0x33);
        assert!(matches!(
            SealedRequest::seal(
                41,
                [0x11; 32],
                [0x44; 16],
                helper(),
                &vec![0; REQUEST_PAYLOAD_BYTES_MAX + 1],
                &secret,
            ),
            Err(BoundaryError::Bound)
        ));
        let exact = request(b"opaque-core-request", &secret);
        for changed in [
            exact.bytes()[..exact.bytes().len() - 1].to_vec(),
            {
                let mut value = exact.bytes().to_vec();
                value.push(0);
                value
            },
            {
                let mut value = exact.bytes().to_vec();
                value[70] ^= 1;
                value
            },
        ] {
            assert!(decode_request(&changed, &secret).is_err());
        }
        assert!(matches!(
            decode_request(exact.bytes(), &OneShotSecret::fixture(0x34)),
            Err(BoundaryError::Authentication)
        ));
    }

    #[test]
    fn response_authentication_rejects_tamper_and_cross_request_replay() {
        let secret = OneShotSecret::fixture(0x33);
        let first = request(b"first", &secret);
        let second = request(b"second", &secret);
        let decoded = decode_request(first.bytes(), &secret).unwrap();
        let response = seal_response(BoundaryStatus::Published, &decoded, &secret);

        assert_eq!(
            verify_response(&response, &first, &secret),
            Ok(BoundaryStatus::Published)
        );
        assert_eq!(
            verify_response(&response, &second, &secret),
            Err(BoundaryError::Replay)
        );
        let mut tampered = response;
        tampered[9] = BoundaryStatus::PublicationFailed as u8;
        assert_eq!(
            verify_response(&tampered, &first, &secret),
            Err(BoundaryError::Authentication)
        );
    }

    #[test]
    fn secret_debug_is_redacted() {
        assert_eq!(
            format!("{:?}", OneShotSecret::fixture(0x7a)),
            "OneShotSecret([redacted])"
        );
    }

    #[test]
    fn outbound_wire_buffer_is_zeroed_when_its_guard_drops() {
        let secret = OneShotSecret::fixture(0x33);
        let request = request(b"opaque-core-request", &secret);
        let mut transport = Vec::new();

        {
            let wire = wire_request(&mut transport, &secret, &request);
            assert_eq!(wire.as_bytes().len(), 32 + request.bytes().len());
        }

        assert!(!transport.is_empty());
        assert!(transport.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn inbound_secret_prefix_is_cleared_when_the_sealed_frame_is_extracted() {
        let secret = OneShotSecret::fixture(0x33);
        let request = request(b"opaque-core-request", &secret);
        let mut inbound = Vec::from(secret.bytes());
        inbound.extend_from_slice(request.bytes());

        let (extracted, sealed): (OneShotSecret, Vec<u8>) =
            split_wire_request(&mut inbound).unwrap();

        assert_eq!(extracted.bytes(), secret.bytes());
        assert_eq!(sealed, request.bytes());
        assert_eq!(inbound.len(), 32);
        assert!(inbound.iter().all(|byte| *byte == 0));
    }
}
