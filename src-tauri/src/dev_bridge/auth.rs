use serde::Deserialize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use subtle::ConstantTimeEq;

/// The first WebSocket text frame is credentials, never an invoke.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Credentials {
    pub pairing_code: Option<String>,
    pub session: Option<String>,
}

pub(super) struct Pairing {
    code: Option<String>,
    session: Option<String>,
    active: bool,
}

impl Pairing {
    pub fn new(code: String) -> Self {
        Self {
            code: Some(code),
            session: None,
            active: false,
        }
    }
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn authenticate(&mut self, credentials: Credentials) -> Result<String, &'static str> {
        if self.active {
            return Err("pairing refused");
        }
        let session = match (credentials.pairing_code, credentials.session) {
            (Some(candidate), None)
                if self
                    .code
                    .as_ref()
                    .is_some_and(|code| constant_eq(code, &candidate)) =>
            {
                let session = random_secret().map_err(|_| "pairing refused")?;
                self.code = None;
                self.session = Some(session.clone());
                session
            }
            (None, Some(candidate))
                if self
                    .session
                    .as_ref()
                    .is_some_and(|session| constant_eq(session, &candidate)) =>
            {
                candidate
            }
            _ => return Err("pairing refused"),
        };
        self.active = true;
        Ok(session)
    }
}

fn constant_eq(expected: &str, candidate: &str) -> bool {
    expected.as_bytes().ct_eq(candidate.as_bytes()).into()
}

pub(super) fn random_secret() -> std::io::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| std::io::Error::other("bridge randomness unavailable"))?;
    use std::fmt::Write;
    let mut result = String::with_capacity(64);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").expect("write string");
    }
    Ok(result)
}

pub(super) fn lock(pairing: &Mutex<Pairing>) -> MutexGuard<'_, Pairing> {
    pairing.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) struct Lease(pub Arc<Mutex<Pairing>>);
impl Drop for Lease {
    fn drop(&mut self) {
        lock(&self.0).active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(value: &str) -> Credentials {
        Credentials {
            pairing_code: Some(value.into()),
            session: None,
        }
    }

    #[test]
    fn pairing_consumes_code_rejects_reuse_and_concurrent_clients_but_allows_reload() {
        let state = Arc::new(Mutex::new(Pairing::new("a".repeat(64))));
        assert!(
            state
                .lock()
                .unwrap()
                .authenticate(code(&"c".repeat(64)))
                .is_err()
        );
        let session = state
            .lock()
            .unwrap()
            .authenticate(code(&"a".repeat(64)))
            .unwrap();
        assert_eq!(session.len(), 64);
        assert!(
            state
                .lock()
                .unwrap()
                .authenticate(Credentials {
                    pairing_code: None,
                    session: Some(session.clone())
                })
                .is_err()
        );
        drop(Lease(state.clone()));
        assert!(
            state
                .lock()
                .unwrap()
                .authenticate(code(&"a".repeat(64)))
                .is_err()
        );
        assert!(
            state
                .lock()
                .unwrap()
                .authenticate(Credentials {
                    pairing_code: None,
                    session: Some("d".repeat(64))
                })
                .is_err()
        );
        assert_eq!(
            state
                .lock()
                .unwrap()
                .authenticate(Credentials {
                    pairing_code: None,
                    session: Some(session.clone())
                })
                .unwrap(),
            session
        );
    }
}
