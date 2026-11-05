use aes_gcm::{
    Aes256Gcm, Key, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use zeroize::Zeroizing;

use super::{VaultError, VaultResult, secret::Secret};

pub(super) const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const ASSOCIATED_DATA_LABEL: &[u8] = b"sage-browser-password-v1";

/// The 256-bit key every saved password is sealed with. It lives on disk only
/// wrapped by the OS (`key_store`).
pub(super) struct VaultKey(Zeroizing<[u8; KEY_LEN]>);

/// What a sealed password belongs to. It is authenticated with the
/// ciphertext, so a blob moved to another row, site or username fails to open.
pub(super) struct Binding<'a> {
    pub(super) id: &'a str,
    pub(super) origin: &'a str,
    pub(super) username: &'a str,
}

impl Binding<'_> {
    fn associated_data(&self) -> Vec<u8> {
        let fields = [self.id, self.origin, self.username];
        let mut data = Vec::with_capacity(
            ASSOCIATED_DATA_LABEL.len() + fields.iter().map(|field| 8 + field.len()).sum::<usize>(),
        );
        data.extend_from_slice(ASSOCIATED_DATA_LABEL);
        for field in fields {
            data.extend_from_slice(&(field.len() as u64).to_le_bytes());
            data.extend_from_slice(field.as_bytes());
        }
        data
    }
}

impl VaultKey {
    pub(super) fn generate() -> VaultResult<Self> {
        let mut key = Zeroizing::new([0; KEY_LEN]);
        getrandom::fill(key.as_mut_slice()).map_err(|_| VaultError::Random)?;
        Ok(Self(key))
    }

    pub(super) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let key: [u8; KEY_LEN] = bytes.try_into().ok()?;
        Some(Self(Zeroizing::new(key)))
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(self.as_bytes()))
    }

    /// Seals `password` under a fresh random nonce; the result is the nonce
    /// followed by the ciphertext and tag.
    pub(super) fn seal(&self, binding: &Binding<'_>, password: &Secret) -> VaultResult<Vec<u8>> {
        let mut nonce = [0; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| VaultError::Random)?;
        let ciphertext = self
            .cipher()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: password.expose().as_bytes(),
                    aad: &binding.associated_data(),
                },
            )
            .map_err(|_| VaultError::Corrupt)?;
        let mut sealed = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&ciphertext);
        Ok(sealed)
    }

    pub(super) fn open(&self, binding: &Binding<'_>, sealed: &[u8]) -> VaultResult<Secret> {
        let (nonce, ciphertext) = sealed
            .split_at_checked(NONCE_LEN)
            .ok_or(VaultError::Corrupt)?;
        let plaintext = self
            .cipher()
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &binding.associated_data(),
                },
            )
            .map_err(|_| VaultError::Corrupt)?;
        String::from_utf8(plaintext)
            .map(Secret::new)
            .map_err(|error| {
                drop(Zeroizing::new(error.into_bytes()));
                VaultError::Corrupt
            })
    }
}
