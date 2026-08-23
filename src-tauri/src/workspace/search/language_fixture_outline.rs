use serde::{Deserialize, Serialize};

use crate::domain::ResourceRef;

use super::LanguageFixtureRequest;

const MAX_OUTLINE_SYMBOLS: usize = 64;
const MAX_SYMBOL_NAME_BYTES: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LanguageOutlineSymbol {
    pub(crate) resource: ResourceRef,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) start_line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LanguageOutlineReport {
    pub(crate) resource: ResourceRef,
    pub(crate) generation: u32,
    pub(crate) symbols: Vec<LanguageOutlineSymbol>,
}

pub(super) fn outline_report(request: &LanguageFixtureRequest) -> LanguageOutlineReport {
    let symbols = request
        .document
        .find("function ")
        .and_then(|offset| {
            let name = request.document[offset + "function ".len()..]
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'$')
                .collect::<Vec<_>>();
            (!name.is_empty()).then(|| LanguageOutlineSymbol {
                resource: request.resource.clone(),
                name: String::from_utf8(name).unwrap_or_default(),
                kind: "function".into(),
                start_line: request.document[..offset]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count() as u32
                    + 1,
            })
        })
        .into_iter()
        .collect();
    LanguageOutlineReport {
        resource: request.resource.clone(),
        generation: request.generation,
        symbols,
    }
}

pub(super) fn valid_outline_report(report: &LanguageOutlineReport) -> bool {
    report.symbols.len() <= MAX_OUTLINE_SYMBOLS
        && report.symbols.iter().all(|symbol| {
            symbol.resource == report.resource
                && !symbol.name.is_empty()
                && symbol.name.len() <= MAX_SYMBOL_NAME_BYTES
                && matches!(
                    symbol.kind.as_str(),
                    "file" | "function" | "type" | "unknown"
                )
                && symbol.start_line > 0
        })
}
