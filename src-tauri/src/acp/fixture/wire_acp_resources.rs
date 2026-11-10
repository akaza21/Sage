//! The embedded resources (ACP `resource` content blocks) a `session/prompt`
//! carried, for a journey whose agent advertises
//! `promptCapabilities.embeddedContext`. The agent answers each prompt with
//! what it received, as an agent that read the files would: each resource's
//! name, type, size and SHA-256, never its content, so a transcript shows
//! which file reached the agent and a test can match it to the staged bytes.

use std::borrow::Cow;

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::super::FixtureError;

/// Resources described one by one; any beyond are only counted, so the echo
/// stays bounded however many blocks a prompt carries.
const DESCRIBED: usize = 16;

/// Present once `initialize` advertised embedded context.
#[derive(Default)]
pub(in crate::acp::fixture) struct EmbeddedContext {
    echo: Option<String>,
}

impl EmbeddedContext {
    /// Reads the resource blocks of one prompt's content. A resource block
    /// without a URI, or without exactly one of `text` and `blob`, is
    /// refused as malformed ACP.
    pub(super) fn receive(&mut self, prompt: &Value) -> Result<(), FixtureError> {
        let blocks = prompt.as_array().ok_or(FixtureError::InvalidArguments)?;
        let mut described = Vec::new();
        let mut count = 0_usize;
        for block in blocks.iter().filter(|block| block["type"] == "resource") {
            let summary = describe(&block["resource"])?;
            count += 1;
            if described.len() < DESCRIBED {
                described.push(summary);
            }
        }
        self.echo = Some(match count {
            0 => "Synthetic replay: the prompt carried no embedded resource.".to_owned(),
            1 => format!(
                "Synthetic replay: the prompt carried 1 embedded resource: {}.",
                described[0]
            ),
            _ if count > described.len() => format!(
                "Synthetic replay: the prompt carried {count} embedded resources: {}; and {} more.",
                described.join("; "),
                count - described.len()
            ),
            _ => format!(
                "Synthetic replay: the prompt carried {count} embedded resources: {}.",
                described.join("; ")
            ),
        });
        Ok(())
    }

    /// What the latest prompt carried, once.
    pub(super) fn take_echo(&mut self) -> Option<String> {
        self.echo.take()
    }
}

fn describe(resource: &Value) -> Result<String, FixtureError> {
    let uri = resource["uri"]
        .as_str()
        .ok_or(FixtureError::InvalidArguments)?;
    let bytes: Cow<'_, [u8]> = match (resource["text"].as_str(), resource["blob"].as_str()) {
        (Some(text), None) => Cow::Borrowed(text.as_bytes()),
        (None, Some(blob)) => Cow::Owned(
            base64::engine::general_purpose::STANDARD
                .decode(blob)
                .map_err(|_| FixtureError::InvalidArguments)?,
        ),
        _ => return Err(FixtureError::InvalidArguments),
    };
    // `mimeType` is optional in ACP; the name is the URI's last segment.
    let mime = resource["mimeType"].as_str().unwrap_or("no type");
    let name = uri
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(uri);
    Ok(format!(
        "{name} ({mime}, {} bytes, SHA-256 {:x})",
        bytes.len(),
        Sha256::digest(&bytes)
    ))
}
