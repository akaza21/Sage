use std::path::Path;

/// Conservative key for duplicate/overlap refusal, never root acceptance.
/// Folding case on Windows can only refuse more. Length-independent verbatim
/// prefixes keep component ancestry stable without erasing literal components.
pub fn conservative_overlap_key(path: &Path) -> String {
    let key = sage_core::win32_path::authority_path_key(path);
    #[cfg(windows)]
    {
        key.to_string_lossy().replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    key.to_string_lossy().into_owned()
}
