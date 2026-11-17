//! Source locations such as `src/token.rs:42`, which read like `key:value`
//! assignments but cite a line of a file.

use std::iter::Peekable;

use super::tokens::work;

/// Whether `key:value` is a source location such as `src/token.rs:42`,
/// `token.rs:42:7` or `token.rs:42-50` rather than a secret: the key ends in a
/// file name with a known source or text extension and the value is only a
/// line, column or range. A dotted key such as `db.password` or `auth.token`
/// names a field, not a file type, so its value is still judged as a secret.
pub(crate) fn file_reference(key: &str, value: &str) -> bool {
    let name = key.rsplit(['/', '\\']).next().unwrap_or(key);
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty()
            && SOURCE_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
    }) && line_reference(value)
}

/// Extensions of files a chat cites by line. None may name a credential
/// field, so a sensitive key name always outranks a file reading.
pub(crate) const SOURCE_EXTENSIONS: &[&str] = &[
    "astro",
    "bash",
    "bat",
    "c",
    "cc",
    "cfg",
    "cjs",
    "clj",
    "cljs",
    "cmake",
    "cmd",
    "conf",
    "cpp",
    "cs",
    "css",
    "csv",
    "cts",
    "cxx",
    "dart",
    "elm",
    "erl",
    "ex",
    "exs",
    "fish",
    "fs",
    "fsx",
    "go",
    "gql",
    "gradle",
    "graphql",
    "h",
    "hcl",
    "hh",
    "hpp",
    "hrl",
    "hs",
    "htm",
    "html",
    "hxx",
    "ini",
    "ipynb",
    "java",
    "jl",
    "js",
    "json",
    "json5",
    "jsonc",
    "jsx",
    "kt",
    "kts",
    "less",
    "lock",
    "log",
    "lua",
    "m",
    "md",
    "mdx",
    "mjs",
    "mk",
    "ml",
    "mli",
    "mm",
    "mts",
    "nim",
    "nix",
    "php",
    "pl",
    "pm",
    "properties",
    "proto",
    "ps1",
    "psm1",
    "py",
    "pyi",
    "r",
    "rb",
    "rs",
    "rst",
    "sass",
    "scala",
    "scss",
    "sh",
    "sol",
    "sql",
    "svelte",
    "svg",
    "swift",
    "tex",
    "tf",
    "toml",
    "ts",
    "tsv",
    "tsx",
    "txt",
    "vb",
    "vim",
    "vue",
    "xml",
    "yaml",
    "yml",
    "zig",
    "zsh",
];

/// Whether `value` is `line` or `line:column`, optionally with a `-end` range,
/// each a number a `u32` line index holds, then only closing punctuation.
/// Parsing stops at the first byte that does not fit, so the work is bounded
/// by the reference rather than by the text that follows it.
fn line_reference(value: &str) -> bool {
    let mut bytes = value
        .bytes()
        .inspect(|_| work::record(|work| work.reference_bytes += 1))
        .peekable();
    if !line_number(&mut bytes) {
        return false;
    }
    for delimiter in *b":-" {
        if bytes.next_if_eq(&delimiter).is_some() && !line_number(&mut bytes) {
            return false;
        }
    }
    bytes.all(|byte| matches!(byte, b'.' | b',' | b';' | b')' | b']' | b'!' | b'?'))
}

fn line_number(bytes: &mut Peekable<impl Iterator<Item = u8>>) -> bool {
    let mut digits = 0usize;
    let mut number = Some(0u32);
    while let Some(digit) = bytes.next_if(u8::is_ascii_digit) {
        digits += 1;
        number =
            number.and_then(|number| number.checked_mul(10)?.checked_add(u32::from(digit - b'0')));
    }
    digits > 0 && number.is_some()
}
