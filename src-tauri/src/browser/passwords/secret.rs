use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;

/// A plaintext password. It has no `Debug`, `Display` or `Serialize`, so it
/// cannot reach a log, a trace or an IPC reply by accident, and its buffer is
/// wiped when it drops.
pub(crate) struct Secret(Zeroizing<String>);

impl Secret {
    pub(crate) fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}
