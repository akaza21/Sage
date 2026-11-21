use super::*;
use crate::acp::slash_commands::{CommandDispatch, CommandSource};
use serde_json::json;

#[test]
fn reported_commands_receive_versioned_capability_gated_placement() {
    let table = KnownCommandTable::parse(
        br#"[
        {"name":"compact","aliases":["summarize"],"section":"session","control":"compact"},
        {"name":"future","section":"usage","since":"3.0.0"},
        {"name":"settings","section":"modelAndAccess","terminalOnly":true}
    ]"#,
    )
    .unwrap();
    let version = ReleaseVersion::parse("cli 2.1.322");
    for (name, aliases, source, can_drive, expected) in [
        (
            "compact",
            vec![],
            CommandSource::Builtin,
            true,
            Some(json!({"section":"session","control":"compact"})),
        ),
        (
            "summarize",
            vec![],
            CommandSource::Harness,
            true,
            Some(json!({"section":"session","control":"compact"})),
        ),
        (
            "shorten",
            vec!["compact"],
            CommandSource::Harness,
            true,
            Some(json!({"section":"session","control":"compact"})),
        ),
        (
            "compact",
            vec![],
            CommandSource::Builtin,
            false,
            Some(json!({"section":"session"})),
        ),
        (
            "custom",
            vec![],
            CommandSource::Harness,
            true,
            Some(json!({"section":"more"})),
        ),
        (
            "compact",
            vec![],
            CommandSource::Skill,
            true,
            Some(json!({"section":"more"})),
        ),
        ("future", vec![], CommandSource::Builtin, true, None),
    ] {
        let command =
            AvailableCommand::new(name, "reported", None, source, CommandDispatch::Prompt)
                .unwrap()
                .with_aliases(aliases);
        let placed = placement(&command, &table, version.as_ref(), &|_| can_drive);
        assert_eq!(
            placed.map(|p| serde_json::to_value(p).unwrap()),
            expected,
            "{name}"
        );
    }
    let command = AvailableCommand::new(
        "future",
        "reported",
        None,
        CommandSource::Builtin,
        CommandDispatch::Prompt,
    )
    .unwrap();
    assert!(placement(&command, &table, None, &|_| true).is_none());
    assert!(place_commands(Vec::new(), &table, version.as_ref(), &|_| true).is_empty());
}

#[test]
fn curated_metadata_rejects_malformed_or_over_cap_tables() {
    for bytes in [
        br#"[{"name":"x","section":"invented"}]"#.to_vec(),
        br#"[{"name":"x","section":"session","control":"invented"}]"#.to_vec(),
        br#"[{"name":"x","section":"session","since":"latest"}]"#.to_vec(),
        br#"[{"name":"bad name","section":"session"}]"#.to_vec(),
        br#"[{"name":"x","section":"session","terminalOnly":false}]"#.to_vec(),
        br#"[{"name":"x","section":"session","extra":true}]"#.to_vec(),
        br#"[{"name":"x","aliases":["x"],"section":"session"}]"#.to_vec(),
        serde_json::to_vec(
            &(0..129)
                .map(|i| json!({"name":format!("cmd{i}"),"section":"session"}))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        vec![b' '; MAX_TABLE_BYTES + 1],
    ] {
        assert!(KnownCommandTable::parse(&bytes).is_err());
    }
    for bytes in [
        include_bytes!("../../../../claude/known_commands.json").as_slice(),
        include_bytes!("../../../../codex/known_commands.json").as_slice(),
        include_bytes!("../../../../pi/known_commands.json").as_slice(),
    ] {
        let table = KnownCommandTable::parse(bytes).unwrap();
        assert!(!table.0.is_empty());
        assert!(table.0.len() <= 128);
        let retained = std::mem::size_of::<KnownCommandTable>()
            + table.0.capacity() * std::mem::size_of::<KnownCommand>()
            + table
                .0
                .iter()
                .map(|entry| {
                    entry.name.capacity()
                        + entry.aliases.capacity() * std::mem::size_of::<String>()
                        + entry.aliases.iter().map(String::capacity).sum::<usize>()
                        + entry.since.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>();
        println!(
            "curated command metadata: entries={}, retained={retained} bytes",
            table.0.len()
        );
    }
    println!(
        "placement={} bytes per command; optional running version={} bytes per Claude runtime",
        std::mem::size_of::<CommandPlacement>(),
        std::mem::size_of::<Option<ReleaseVersion>>()
    );
}

#[test]
fn curated_commands_survive_a_full_report_ahead_of_other_builtins() {
    use crate::acp::slash_commands::{CommandList, parse_acp};
    let table = KnownCommandTable::parse(
        br#"[{"name":"compact","section":"session","control":"compact"}]"#,
    )
    .unwrap();
    let mut report: Vec<_> = (0..512).map(|i| json!({"name":format!("cmd{i}"),"description":"reported","_meta":{"source":"builtin"}})).collect();
    report.push(json!({"name":"compact","description":"Compact","_meta":{"source":"builtin"}}));
    let mut list = CommandList::default();
    list.replace(Ok(place_commands(
        parse_acp(&json!(report)).unwrap(),
        &table,
        None,
        &|_| true,
    )))
    .unwrap();
    assert_eq!((list.commands.len(), list.omitted), (512, 1));
    assert_eq!(
        serde_json::to_value(&list.commands[0]).unwrap()["placement"],
        json!({"section":"session","control":"compact"})
    );
    assert!(list.validate(list.revision, "/compact").is_ok());
    assert!(list.validate(list.revision, "/cmd511").is_err());
}
