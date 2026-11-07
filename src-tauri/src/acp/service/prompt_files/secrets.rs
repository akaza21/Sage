//! Names that hold credentials. They are never attached, even when no ignore
//! rule excludes them (a home folder has no repository to supply rules).

/// Folders whose whole content is credential material.
const FOLDERS: &[&str] = &[".ssh", ".gnupg", ".aws", ".azure", ".kube", ".docker"];
/// Exact file names of credential stores.
const FILES: &[&str] = &[
    ".netrc",
    "_netrc",
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    "credentials",
    ".pgpass",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
];
/// Extensions of private keys, certificates with keys and key stores.
const EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "jks", "keystore", "kdbx", "ppk"];

/// Whether `name`, one path component, is a credential folder or file.
/// Matching ignores ASCII case, as the file systems that matter here do.
pub(super) fn is_secret(name: &str, is_dir: bool) -> bool {
    let name = name.to_ascii_lowercase();
    if is_dir {
        return FOLDERS.contains(&name.as_str());
    }
    name == ".env"
        || name.starts_with(".env.")
        || FILES.contains(&name.as_str())
        || name
            .rsplit_once('.')
            .is_some_and(|(stem, ext)| !stem.is_empty() && EXTENSIONS.contains(&ext))
}

/// Whether any component of `path` names a credential folder, or its final
/// component a credential file.
pub(super) fn path_is_secret(path: &std::path::Path, is_dir: bool) -> bool {
    let mut components = path.components().filter_map(|c| match c {
        std::path::Component::Normal(name) => name.to_str(),
        _ => None,
    });
    let last = components.next_back();
    components.any(|name| is_secret(name, true))
        || last.is_some_and(|name| is_secret(name, true) || is_secret(name, is_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_names_are_secret_and_ordinary_names_are_not() {
        for file in [
            ".env",
            ".env.local",
            ".ENV.production",
            "credentials",
            ".git-credentials",
            "id_ed25519",
            "server.pem",
            "Signing.P12",
            "vault.kdbx",
            ".netrc",
        ] {
            assert!(is_secret(file, false), "{file}");
        }
        for dir in [".ssh", ".AWS", ".gnupg"] {
            assert!(is_secret(dir, true), "{dir}");
        }
        for file in [
            "id_ed25519.pub",
            "notes.txt",
            "keys.md",
            "environment.ts",
            ".pem",
        ] {
            assert!(!is_secret(file, false), "{file}");
        }
        assert!(!is_secret("src", true));
    }
}
