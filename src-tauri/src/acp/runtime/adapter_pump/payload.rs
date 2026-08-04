use super::*;
use sha2::{Digest, Sha256};

pub(super) struct PayloadMeasurement {
    pub digest: String,
    pub bytes: usize,
}

struct PayloadWriter {
    bytes: usize,
    maximum: usize,
    digest: Sha256,
    retained: Option<Vec<u8>>,
}

impl PayloadWriter {
    fn new(maximum: usize, retain: bool) -> Self {
        Self {
            bytes: 0,
            maximum,
            digest: Sha256::new(),
            retained: retain.then(Vec::new),
        }
    }

    fn finish(self) -> Result<(Option<String>, PayloadMeasurement), AdapterPumpError> {
        let json = self
            .retained
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| AdapterPumpError::BatchCapacity)?;
        Ok((
            json,
            PayloadMeasurement {
                digest: hex(&self.digest.finalize()),
                bytes: self.bytes,
            },
        ))
    }
}

impl std::io::Write for PayloadWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let next = self
            .bytes
            .checked_add(bytes.len())
            .filter(|next| *next <= self.maximum)
            .ok_or_else(|| std::io::Error::other("normalized payload capacity"))?;
        if let Some(retained) = self.retained.as_mut() {
            if retained.capacity() < next {
                let doubled = retained.capacity().saturating_mul(2).max(1024);
                let target = next.max(doubled).min(self.maximum);
                retained
                    .try_reserve_exact(target - retained.capacity())
                    .map_err(|_| std::io::Error::other("normalized payload allocation"))?;
            }
            retained.extend_from_slice(bytes);
        }
        self.digest.update(bytes);
        self.bytes = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn payload_measure(
    payload: &NormalizedPayload,
) -> Result<PayloadMeasurement, AdapterPumpError> {
    let mut writer = PayloadWriter::new(LIMITS.wire.normalized_item_bytes as usize, false);
    serde_json::to_writer(&mut writer, payload).map_err(|_| AdapterPumpError::BatchCapacity)?;
    Ok(writer.finish()?.1)
}

pub(crate) fn normalized_payload_bytes(
    payload: &NormalizedPayload,
) -> Result<usize, AdapterPumpError> {
    payload_measure(payload).map(|measurement| measurement.bytes)
}

pub(super) fn payload_json(
    payload: &NormalizedPayload,
) -> Result<(String, PayloadMeasurement), AdapterPumpError> {
    let mut writer = PayloadWriter::new(LIMITS.wire.permission_payload_bytes as usize, true);
    serde_json::to_writer(&mut writer, payload).map_err(|_| AdapterPumpError::BatchCapacity)?;
    let (json, measurement) = writer.finish()?;
    Ok((json.ok_or(AdapterPumpError::BatchCapacity)?, measurement))
}
