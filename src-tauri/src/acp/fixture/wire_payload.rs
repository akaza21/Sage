use std::io::Write;

use serde_json::Value;

use crate::acp::limits::LIMITS;

use super::{
    FixtureError,
    interpreter::{field_str, field_u64},
    wire::{WireState, emit_repeated, write_stdout},
};

pub(super) fn emit_bounded_json(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let shape = field_str(step, "shape")?;
    let amount = step
        .get("count")
        .or_else(|| step.get("bytes"))
        .or_else(|| step.get("depth"))
        .and_then(Value::as_u64)
        .ok_or(FixtureError::InvalidArguments)?;
    if amount > LIMITS.fixture.expanded_bytes_per_case {
        return Err(FixtureError::ExpandedLimit);
    }
    let amount = usize::try_from(amount).map_err(|_| FixtureError::ExpandedLimit)?;
    let value = match shape {
        "nestedObject" => format!("{}0{}", "{\"x\":".repeat(amount), "}".repeat(amount)),
        "objectMembers" => {
            let members = (0..amount)
                .map(|index| format!("\"k{index}\":0"))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{members}}}")
        }
        "arrayEntries" => format!("{{\"values\":[{}]}}", vec!["0"; amount].join(",")),
        "nodeCount" => {
            if amount < 256 {
                return Err(FixtureError::InvalidArguments);
            }
            let mut scalar_nodes = amount - 256;
            let members = (0..256)
                .map(|index| {
                    let remaining_arrays = 256 - index;
                    let entries = scalar_nodes.div_ceil(remaining_arrays);
                    scalar_nodes -= entries;
                    format!("\"k{index}\":[{}]", vec!["0"; entries].join(","))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{members}}}")
        }
        "decodedKeyBytes" => format!("{{\"{}\":0}}", "k".repeat(amount)),
        "numericTokenBytes" => format!("{{\"id\":{}}}", "1".repeat(amount)),
        _ => return Err(FixtureError::InvalidArguments),
    };
    write_stdout(output, state, value.as_bytes(), false)?;
    write_stdout(output, state, b"\n", false)
}

pub(super) fn emit_sized_string(
    name: &str,
    bytes: u64,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let prefix =
        format!("{{\"jsonrpc\":\"2.0\",\"method\":\"fixture/{name}\",\"params\":{{\"value\":\"");
    write_stdout(output, state, prefix.as_bytes(), false)?;
    emit_repeated(output, state, b'x', bytes)?;
    write_stdout(output, state, b"\"}}\n", false)
}

pub(super) fn emit_capability_token(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    if field_str(step, "document")? != "canonicalLegacyDenyAll" {
        return Err(FixtureError::InvalidArguments);
    }
    let document = br#"{"schemaVersion":1,"protocolVersion":"legacy","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#;
    let total = field_u64(step, "totalTokenBytes")?;
    let padding = total
        .checked_sub(document.len() as u64)
        .ok_or(FixtureError::InvalidArguments)?;
    write_stdout(output, state, document, false)?;
    emit_repeated(output, state, b' ', padding)?;
    write_stdout(output, state, b"\n", false)
}

pub(super) fn emit_sized_method(
    bytes: u64,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    write_stdout(output, state, b"{\"jsonrpc\":\"2.0\",\"method\":\"", false)?;
    emit_repeated(output, state, b'm', bytes)?;
    write_stdout(output, state, b"\"}\n", false)
}

pub(super) fn emit_metadata(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let entries = field_u64(step, "entries")?;
    let sizes = semantic_entry_sizes(step, entries)?;
    let keys = metadata_keys(entries)?;
    let mut metadata = serde_json::Map::new();
    for (key, bytes) in keys.into_iter().zip(sizes) {
        let value_bytes = bytes
            .checked_sub(key.len() as u64)
            .filter(|value| *value > 0)
            .ok_or(FixtureError::InvalidArguments)?;
        metadata.insert(key, Value::String("v".repeat(value_bytes as usize)));
    }
    emit_json(
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "fixture/metadata",
            "params": {"_meta": metadata},
        }),
        state,
        output,
    )
}

pub(super) fn emit_models(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let entries = field_u64(step, "entries")?;
    let sizes = model_entry_sizes(step, entries)?;
    let mut models = Vec::new();
    models
        .try_reserve_exact(entries as usize)
        .map_err(|_| FixtureError::ExpandedLimit)?;
    for (index, (id_bytes, name_bytes)) in sizes.into_iter().enumerate() {
        if id_bytes == 0 || name_bytes == 0 || id_bytes > 512 || name_bytes > 512 {
            return Err(FixtureError::InvalidArguments);
        }
        let mut id = format!("m{index:x}");
        if id.len() > id_bytes as usize {
            id.truncate(id_bytes as usize);
        } else {
            id.push_str(&"i".repeat(id_bytes as usize - id.len()));
        }
        models.push(serde_json::json!({
            "id": id,
            "name": "n".repeat(name_bytes as usize),
        }));
    }
    emit_json(
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "fixture/models",
            "params": {"models": models},
        }),
        state,
        output,
    )
}

fn semantic_entry_sizes(step: &Value, entries: u64) -> Result<Vec<u64>, FixtureError> {
    if let Some(values) = step.get("entryBytes").and_then(Value::as_array) {
        if values.len() != entries as usize {
            return Err(FixtureError::InvalidArguments);
        }
        return values
            .iter()
            .map(|value| value.as_u64().ok_or(FixtureError::InvalidArguments))
            .collect();
    }
    let total = field_u64(step, "aggregateBytes")?;
    if entries == 0 || total < entries.checked_mul(2).ok_or(FixtureError::ExpandedLimit)? {
        return Err(FixtureError::InvalidArguments);
    }
    let base = total / entries;
    let extra = total % entries;
    Ok((0..entries)
        .map(|index| base + u64::from(index < extra))
        .collect())
}

fn model_entry_sizes(step: &Value, entries: u64) -> Result<Vec<(u64, u64)>, FixtureError> {
    if let Some(repeated) = step.get("repeatedEntry") {
        let id = field_u64(repeated, "idBytes")?;
        let name = field_u64(repeated, "nameBytes")?;
        let count = field_u64(repeated, "count")?;
        let mut result = vec![(id, name); count as usize];
        if let Some(values) = step.get("entryBytes").and_then(Value::as_array) {
            for total in values {
                let total = total.as_u64().ok_or(FixtureError::InvalidArguments)?;
                if !(2..=1024).contains(&total) {
                    return Err(FixtureError::InvalidArguments);
                }
                let name = (total / 2).min(512);
                result.push((total - name, name));
            }
        }
        if result.len() != entries as usize {
            return Err(FixtureError::InvalidArguments);
        }
        return Ok(result);
    }
    semantic_entry_sizes(step, entries).and_then(|sizes| {
        sizes
            .into_iter()
            .map(|total| {
                let name = total / 2;
                let id = total - name;
                if id == 0 || name == 0 || id > 512 || name > 512 {
                    Err(FixtureError::InvalidArguments)
                } else {
                    Ok((id, name))
                }
            })
            .collect()
    })
}

fn metadata_keys(entries: u64) -> Result<Vec<String>, FixtureError> {
    let keys = (b'!'..=b'~')
        .filter(|byte| !matches!(byte, b'"' | b'\\'))
        .take(entries as usize)
        .map(|byte| char::from(byte).to_string())
        .collect::<Vec<_>>();
    if keys.len() != entries as usize {
        return Err(FixtureError::InvalidArguments);
    }
    Ok(keys)
}

fn emit_json(
    value: Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let encoded = serde_json::to_vec(&value).map_err(|_| FixtureError::InvalidArguments)?;
    write_stdout(output, state, &encoded, false)?;
    write_stdout(output, state, b"\n", false)
}
