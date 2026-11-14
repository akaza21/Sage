use sage_core::error::{AppError, AppResult};
use sha2::{Digest, Sha256};

pub const MAX_EDITABLE_BYTES: u64 = 1024 * 1024;

pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

pub(crate) fn validate_line_ending(line_ending: &str) -> AppResult<()> {
    if matches!(line_ending, "lf" | "crlf" | "none") {
        Ok(())
    } else {
        Err(AppError::InvalidRequest(
            "line ending must be lf, crlf, or none".into(),
        ))
    }
}

pub(crate) fn detect_line_ending(content: &str) -> AppResult<&'static str> {
    let bytes = content.as_bytes();
    let mut crlf = 0_u64;
    let mut bare_lf = 0_u64;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            if index > 0 && bytes[index - 1] == b'\r' {
                crlf += 1;
            } else {
                bare_lf += 1;
            }
        }
    }
    if crlf > 0 && bare_lf > 0 {
        return Err(AppError::MixedLineEndings);
    }
    Ok(if crlf > 0 {
        "crlf"
    } else if bare_lf > 0 {
        "lf"
    } else {
        "none"
    })
}

pub(crate) fn canonicalize_editor_content(content: &str) -> String {
    content.replace("\r\n", "\n")
}

pub(crate) fn materialize_content(content: &str, line_ending: &str) -> String {
    if line_ending == "crlf" {
        content.replace('\n', "\r\n")
    } else {
        content.into()
    }
}

pub(crate) fn reject_unsupported_content(content: &str) -> AppResult<()> {
    if content.as_bytes().iter().take(8_192).any(|byte| *byte == 0) {
        return Err(AppError::InvalidRequest(
            "binary content cannot be persisted in the editor".into(),
        ));
    }
    if content.len() as u64 > MAX_EDITABLE_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "buffer exceeds the {} MiB editor limit",
            MAX_EDITABLE_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

pub(crate) fn reject_oversized_materialized(content: &str) -> AppResult<()> {
    if content.len() as u64 > MAX_EDITABLE_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "encoded buffer exceeds the {} MiB editor limit",
            MAX_EDITABLE_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{hint::black_box, time::Instant};

    use super::*;

    #[test]
    #[ignore = "manual release-mode performance evidence"]
    fn benchmarks_sha256_for_one_mebibyte() {
        let bytes = vec![0x5a; MAX_EDITABLE_BYTES as usize];
        for _ in 0..20 {
            black_box(fingerprint(black_box(&bytes)));
        }
        let mut elapsed_micros = Vec::with_capacity(200);
        for _ in 0..200 {
            let started = Instant::now();
            black_box(fingerprint(black_box(&bytes)));
            elapsed_micros.push(started.elapsed().as_micros());
        }
        elapsed_micros.sort_unstable();
        println!(
            "sha256_1mib iterations=200 p50_us={} p95_us={} max_us={}",
            elapsed_micros[99], elapsed_micros[189], elapsed_micros[199]
        );
    }
}
