//! Output that did not fit the batch being built, drained in the batches
//! after it.

use std::collections::VecDeque;

use super::*;

enum Held {
    Event(Box<AdapterEvent>),
    Egress(AdapterEgress),
}

/// Output held back, in order, once a batch filled up, such as the replies a
/// turn's end shows whole. Once anything is held, everything after it is held
/// too, so the turn's terminal event still follows the replies before it.
#[derive(Default)]
pub(crate) struct HeldOutput(VecDeque<Held>);

impl HeldOutput {
    /// Every one of `events`, to drain from the first batch on.
    pub(crate) fn of(events: impl IntoIterator<Item = AdapterEvent>) -> Self {
        Self(
            events
                .into_iter()
                .map(|event| Held::Event(Box::new(event)))
                .collect(),
        )
    }

    /// Pushes `event` into `batch`, or holds it if anything is already held
    /// or `batch` is full.
    pub(crate) fn push_event(
        &mut self,
        batch: &mut AdapterBatchBuilder,
        event: AdapterEvent,
    ) -> Result<(), AdapterRuntimeError> {
        if self.0.is_empty() && batch.fits_event(&event)? {
            return batch.push_event(event);
        }
        self.hold(Held::Event(Box::new(event)))
    }

    /// Pushes `egress` into `batch`, or holds it after what is held.
    pub(crate) fn push_egress(
        &mut self,
        batch: &mut AdapterBatchBuilder,
        egress: AdapterEgress,
    ) -> Result<(), AdapterRuntimeError> {
        if self.0.is_empty() && batch.fits_egress(&egress) {
            return batch.push_egress(egress);
        }
        self.hold(Held::Egress(egress))
    }

    /// The next batch of what is held, as much as fits, or `None` once
    /// everything is drained.
    pub(crate) fn next_batch(&mut self) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        if self.0.is_empty() {
            return Ok(None);
        }
        let mut batch = AdapterBatchBuilder::new();
        while let Some(held) = self.0.front() {
            let fits = match held {
                Held::Event(event) => batch.fits_event(event)?,
                Held::Egress(egress) => batch.fits_egress(egress),
            };
            if !fits {
                break;
            }
            match self.0.pop_front() {
                Some(Held::Event(event)) => batch.push_event(*event)?,
                Some(Held::Egress(egress)) => batch.push_egress(egress)?,
                None => break,
            }
        }
        if batch.is_empty() {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        Ok(Some(batch.finish()))
    }

    fn hold(&mut self, held: Held) -> Result<(), AdapterRuntimeError> {
        self.0
            .try_reserve(1)
            .map_err(|_| AdapterRuntimeError::BatchBytes)?;
        self.0.push_back(held);
        Ok(())
    }
}
