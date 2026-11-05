use super::*;
use crate::repository_public_error::MAX_PUBLIC_REPOSITORY_ERROR_BYTES;

#[test]
fn classifies_command_running_keys() {
    for key in [
        "filter.lfs.clean",
        "filter.Ev.il.smudge",
        "filter.x.process",
        "diff.x.textconv",
        "diff.x.command",
        "diff.external",
        "merge.x.driver",
        "core.sshCommand",
        "core.askpass",
        "core.gitproxy",
        "core.alternateRefsCommand",
        "core.editor",
        "core.pager",
        "credential.helper",
        "credential.https://user:token@host.helper",
        "remote.origin.uploadpack",
        "remote.origin.receivepack",
        "sequence.editor",
        "gpg.program",
        "gpg.ssh.program",
        "gpg.ssh.defaultKeyCommand",
        "include.path",
        "includeIf.gitdir:/x/.path",
    ] {
        let parsed = ConfigKey::parse(key.as_bytes()).unwrap();
        assert!(parsed.runs_command(), "{key} should be refused");
    }
    for key in [
        "core.hooksPath",
        "core.fsmonitor",
        "core.bare",
        "filter.lfs.required",
        "filter.clean",
        "diff.x.binary",
        "diff.algorithm",
        "merge.x.name",
        "merge.conflictStyle",
        "remote.origin.url",
        "remote.uploadpack",
        "user.name",
        "branch.main.remote",
        "sequence.x.editor",
    ] {
        let parsed = ConfigKey::parse(key.as_bytes()).unwrap();
        assert!(!parsed.runs_command(), "{key} should be allowed");
    }
    assert_eq!(ConfigKey::parse(b"nodot"), None);
    assert_eq!(ConfigKey::parse(b".name"), None);
}

#[test]
fn refusal_names_bounded_redacted_keys() {
    let listing = b"core.bare\0credential.https://user:token@host.helper\0filter.lfs.clean\0\
filter.lfs.clean\0filter.lfs.smudge\0merge.x\x1b[2J.driver\0"
        .to_vec();
    let message = refusal_message(&listing).unwrap();
    assert!(message.contains("credential.*.helper, filter.lfs.clean, filter.lfs.smudge"));
    assert!(message.contains("and 1 more"), "{message}");
    assert!(!message.contains("token"));
    assert!(!message.contains('\x1b'));

    let worst = (0..8)
        .map(|index| {
            let padding = "p".repeat(MAX_SUBSECTION_CHARS - 1);
            format!("filter.{index}{padding}.process\0")
        })
        .collect::<String>();
    assert!(refusal_message(worst.as_bytes()).unwrap().len() <= MAX_PUBLIC_REPOSITORY_ERROR_BYTES);
    assert_eq!(refusal_message(b"core.bare\0user.name\0"), None);
}

#[test]
fn path_protection_keys_are_found_in_any_case_and_only_as_whole_keys() {
    let listing = b"core.bare\0core.protectNTFS\0core.protectntfsx\0user.core.protecthfs\0";
    assert_eq!(path_protection_keys(listing), ["core.protectntfs"]);
    assert_eq!(
        path_protection_keys(b"CORE.PROTECTHFS\0core.protectntfs\0"),
        ["core.protectntfs", "core.protecthfs"]
    );
    assert!(path_protection_keys(b"core.bare\0").is_empty());
    let message = path_protection_refusal("core.protectntfs");
    assert!(message.contains("core.protectNTFS"), "{message}");
    assert!(message.len() <= MAX_PUBLIC_REPOSITORY_ERROR_BYTES);
}
