use std::{fmt::Debug, time::Instant};

/// Failure-only, bounded metadata: never include prompts, frames, or paths.
pub(crate) fn failed(stage: &'static str, started: Instant, error: &impl Debug, state: impl Debug) {
    let elapsed_ms = started.elapsed().as_millis();
    tracing::warn!(stage, elapsed_ms, ?error, ?state, "ACP prompt failed");
    // Lib tests do not necessarily install a tracing subscriber. Keep the
    // failing test's captured output useful in that case too.
    #[cfg(test)]
    eprintln!(
        "ACP prompt failed: stage={stage} elapsed_ms={elapsed_ms} error={error:?} state={state:?}"
    );
}
