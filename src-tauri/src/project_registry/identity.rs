use std::path::Path;

/// Identity only: never pass this key to a harness or the filesystem. Windows
/// drive/UNC prefixes use one verbatim spelling at every length, so component
/// ancestry is stable and literal dots, spaces and device names stay intact.
pub fn canonical_comparison_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let text = path.to_string_lossy();
        let identity = match path.components().next() {
            Some(Component::Prefix(prefix)) => {
                let tail = &text[prefix.as_os_str().to_string_lossy().len()..];
                match prefix.kind() {
                    Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                        format!(r"\\?\{}:{tail}", char::from(drive))
                    }
                    Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                        format!(
                            r"\\?\UNC\{}\{}{tail}",
                            server.to_string_lossy(),
                            share.to_string_lossy()
                        )
                    }
                    _ => text.into_owned(),
                }
            }
            _ => text.into_owned(),
        };
        identity.replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    path.to_string_lossy().into_owned()
}
