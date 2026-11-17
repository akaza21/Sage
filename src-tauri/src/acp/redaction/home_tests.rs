use super::*;

fn shorten(home: &str, text: &str) -> String {
    HomePrefix::new(home).unwrap().shorten(text).into_owned()
}

#[test]
fn windows_home_is_shortened_case_and_separator_insensitively() {
    let home = r"C:\Users\alice";
    for (input, expected) in [
        (r"see C:\Users\alice\proj\x.rs now", r"see ~\proj\x.rs now"),
        (r"see c:\users\ALICE\proj", r"see ~\proj"),
        ("see C:/Users/alice/proj", "see ~/proj"),
        (r#"path = "C:\\Users\\alice\\proj""#, r#"path = "~\\proj""#),
        (r"(C:\Users\alice)", "(~)"),
        (r"home is C:\Users\alice.", "home is ~."),
        ("file:///C:/Users/alice/proj/x.rs", "~/proj/x.rs"),
    ] {
        assert_eq!(shorten(home, input), expected, "{input}");
    }
}

#[test]
fn home_name_with_spaces_is_matched_exactly() {
    assert_eq!(
        shorten(
            r"C:\Users\Alice Smith\",
            r"in C:\Users\Alice Smith\a b\c.rs"
        ),
        r"in ~\a b\c.rs"
    );
    assert_eq!(
        shorten("/Users/Alice Smith", "open /Users/Alice Smith/a b/c"),
        "open ~/a b/c"
    );
    assert_eq!(
        shorten("/Users/Alice Smith", "open /Users/Alice Smithers/c"),
        "open /Users/Alice Smithers/c"
    );
}

#[test]
fn posix_home_is_shortened_only_as_a_whole_rooted_prefix() {
    let home = "/home/alice";
    for (input, expected) in [
        ("cd /home/alice/src && ls", "cd ~/src && ls"),
        ("`/home/alice/src`", "`~/src`"),
        ("[x](/home/alice/a.rs:3)", "[x](~/a.rs:3)"),
        ("file:///home/alice/a.png", "~/a.png"),
        ("/home/alice2/src", "/home/alice2/src"),
        ("/home/alice.bak/src", "/home/alice.bak/src"),
        ("/mnt/home/alice/src", "/mnt/home/alice/src"),
        ("/HOME/alice/src", "/HOME/alice/src"),
        (r"/home/alice\x", r"/home/alice\x"),
        ("/home/bob/src /srv/data", "/home/bob/src /srv/data"),
        ("a /home/alice b /home/alice/c", "a ~ b ~/c"),
    ] {
        assert_eq!(shorten(home, input), expected, "{input}");
    }
}

#[test]
fn roots_relative_and_unc_paths_are_not_homes() {
    for home in [
        "",
        "/",
        "//",
        r"C:\",
        "C:/",
        "home/alice",
        "~",
        r"\\server\share\alice",
        r"\\?\UNC\server\share\alice",
        r"\\?\C:\",
    ] {
        assert_eq!(HomePrefix::new(home), None, "{home}");
    }
}

#[test]
fn verbatim_windows_home_is_read_in_its_ordinary_form() {
    let home = HomePrefix::new(r"\\?\C:\Users\alice").unwrap();
    assert_eq!(home, HomePrefix::new(r"C:\Users\alice").unwrap());
    assert_eq!(
        home.shorten(r"see C:\Users\alice\proj\x.rs"),
        r"see ~\proj\x.rs"
    );
    assert_eq!(
        home.expand(r"~\proj\x.rs"),
        Some(PathBuf::from(r"C:\Users\alice\proj\x.rs"))
    );
}

#[test]
fn sibling_folders_sharing_the_home_name_are_left_whole() {
    for (home, separator, siblings) in [
        (
            "/home/alice",
            "/",
            [
                "/home/alice+backup/x.rs",
                "/home/alice@old/x.rs",
                "/home/alice,v2/x.rs",
                "/home/alice)x/x.rs",
                "/home/alice.d/x.rs",
                "/home/alice./x.rs",
            ],
        ),
        (
            r"C:\Users\alice",
            r"\",
            [
                r"C:\Users\alice+backup\x.rs",
                r"C:\Users\alice@old\x.rs",
                r"C:\Users\alice,v2\x.rs",
                r"C:\Users\alice)x\x.rs",
                r"C:\Users\alice.d\x.rs",
                r"C:\Users\alice.\x.rs",
            ],
        ),
    ] {
        let prefix = HomePrefix::new(home).unwrap();
        for sibling in siblings {
            let text = format!("open {sibling} now");
            assert_eq!(prefix.shorten(&text), text, "{sibling}");
        }
        let inside = format!("{home}{separator}proj{separator}x.rs");
        let shortened = prefix.shorten(&inside).into_owned();
        assert_eq!(shortened, format!("~{separator}proj{separator}x.rs"));
        assert_eq!(prefix.expand(&shortened), Some(PathBuf::from(&inside)));
        for (text, expected) in [
            (format!("({home}), then"), "(~), then"),
            (format!("\"{home}\"."), "\"~\"."),
            (format!("**{home}**"), "**~**"),
        ] {
            assert_eq!(prefix.shorten(&text), expected, "{text}");
        }
    }
}

#[test]
fn siblings_whose_names_continue_past_a_space_are_left_whole() {
    for (home, separator) in [(r"C:\Users\Alice", r"\"), ("/home/alice", "/")] {
        let name = home.rsplit(['/', '\\']).next().unwrap();
        let prefix = HomePrefix::new(home).unwrap().with_siblings([
            format!("{name} Smith").as_str(),
            &format!("{name} Jane Smith"),
            &format!("{name}2"),
        ]);
        let sibling = format!("{home} Smith{separator}project{separator}file.rs");
        let long_sibling = format!("{home} Jane Smith{separator}file.rs");
        for text in [
            format!("open {sibling} now"),
            format!("open {long_sibling} now"),
            format!("open {home} Smith now"),
            format!("\"{sibling}\""),
            format!("`{sibling}`"),
            format!("open `{home} Smith` now"),
            format!("open \"{home} Smith\" now"),
            format!("'{home} Smith'"),
            // Folders beside the home folder made after Sage started.
            format!("open {home} Smithson{separator}x now"),
            format!("open {home} Jane Doe{separator}x now"),
            format!("{home} 2024 Backup{separator}x"),
        ] {
            assert_eq!(prefix.shorten(&text), text, "{text}");
        }
        for (text, expected) in [
            (format!("\"{home}\""), "\"~\"".to_owned()),
            (format!("`{home}`"), "`~`".to_owned()),
            (
                format!("`{home}{separator}x.rs`"),
                format!("`~{separator}x.rs`"),
            ),
            (format!("in {home}."), "in ~.".to_owned()),
            (format!("{home} is home"), "~ is home".to_owned()),
            (format!("{home}, then"), "~, then".to_owned()),
            (
                format!("{home} then {home}{separator}x"),
                format!("~ then ~{separator}x"),
            ),
            (
                format!("{home}\nSmith{separator}x"),
                format!("~\nSmith{separator}x"),
            ),
            (
                format!("Cloned into {home} and opened src{separator}main.rs"),
                format!("Cloned into ~ and opened src{separator}main.rs"),
            ),
            (format!("{home} Smithson now"), "~ Smithson now".to_owned()),
        ] {
            assert_eq!(prefix.shorten(&text), expected, "{text}");
        }
    }
}

#[test]
fn shortened_paths_expand_back_under_the_home_folder() {
    let posix = HomePrefix::new("/home/alice/").unwrap();
    assert_eq!(
        posix.expand("~/proj/a.rs"),
        Some(PathBuf::from("/home/alice/proj/a.rs"))
    );
    assert_eq!(posix.expand("~"), Some(PathBuf::from("/home/alice")));
    assert_eq!(posix.expand(r"~\proj"), None);
    assert_eq!(posix.expand("~bob/proj"), None);
    assert_eq!(posix.expand("proj/a.rs"), None);
    for home in [r"C:\Users\alice", "C:/Users/alice/"] {
        let windows = HomePrefix::new(home).unwrap();
        for shortened in [r"~\proj\x.rs", "~/proj/x.rs", r"~/proj\x.rs"] {
            assert_eq!(
                windows.expand(shortened),
                Some(PathBuf::from(r"C:\Users\alice\proj\x.rs")),
                "{home} {shortened}"
            );
        }
        assert_eq!(windows.expand("~"), Some(PathBuf::from(r"C:\Users\alice")));
    }
}

#[test]
fn the_current_home_leaves_reading_the_folders_beside_it_to_startup() {
    if let Some(home) = HomePrefix::current() {
        assert_eq!(home.siblings, None);
    }
}
