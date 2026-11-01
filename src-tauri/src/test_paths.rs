use std::path::PathBuf;

/// Resolve linked temp ancestors and 8.3 names before recording fixture paths.
/// Keep the ordinary DOS spelling required by agent tool paths and Codex packets.
pub(crate) fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir()
        .canonicalize()
        .expect("canonical temporary directory");
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
        let unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
        let ordinary = if let Some(rest) = wide.strip_prefix(unc.as_slice()) {
            [vec![u16::from(b'\\'); 2], rest.to_vec()].concat()
        } else {
            wide.strip_prefix(verbatim.as_slice())
                .unwrap_or(&wide)
                .to_vec()
        };
        PathBuf::from(OsString::from_wide(&ordinary))
    }
    #[cfg(not(windows))]
    {
        path
    }
}
