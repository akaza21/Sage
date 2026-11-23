use super::{Probed, compare};
use crate::acp::{resource_mapper::ContentState, store::agent_writes::LastWrites};

#[test]
fn compare_sorts_each_file_by_the_agents_last_write() {
    const AGENT: [u8; 32] = [1; 32];
    const ROOT: &str = "root-a";
    let writes = LastWrites::from([
        ((ROOT.into(), "match.rs".into()), Some(AGENT)),
        ((ROOT.into(), "edited.rs".into()), Some(AGENT)),
        ((ROOT.into(), "missing.rs".into()), Some(AGENT)),
        ((ROOT.into(), "null.rs".into()), None),
    ]);
    // (file, mapped to a root, bytes now, changed, unverified)
    let table = [
        ("match.rs", true, ContentState::Present(AGENT), false, false),
        ("edited.rs", true, ContentState::Present([2; 32]), true, false),
        ("missing.rs", true, ContentState::Missing, true, false),
        ("null.rs", true, ContentState::Present(AGENT), false, true),
        ("unrecorded.rs", true, ContentState::Present(AGENT), false, true),
        ("unreadable.rs", true, ContentState::Unreadable, false, true),
        ("unmapped.rs", false, ContentState::Present(AGENT), false, true),
    ];
    for (file, mapped, state, changed, unverified) in table {
        let preview = compare(
            vec![Probed {
                shown: file.into(),
                key: mapped.then(|| (ROOT.into(), file.into())),
                state,
            }],
            &writes,
        );
        assert_eq!(preview.files, [file], "{file}");
        assert_eq!(preview.conflicts.contains(&file.to_owned()), changed, "{file}");
        assert_eq!(preview.unverified.contains(&file.to_owned()), unverified, "{file}");
        assert_eq!(preview.refusal().is_some(), changed || unverified, "{file}");
    }
}
