use super::*;
use std::{fs, path::PathBuf};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-mentions-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> &Self {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        self
    }
    fn workspace(&self, id: &str) -> Workspace {
        Workspace::open("project", id, &self.0).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tokens(text: &str) -> Vec<String> {
    mention_tokens(text)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn blocks(mentions: &Mentions, embedded: bool) -> Vec<serde_json::Value> {
    mentions.blocks(embedded).collect()
}

#[test]
fn tokens_start_a_word_and_quoted_tokens_keep_their_spaces() {
    assert_eq!(
        mention_tokens("@a.rs see bob@example.com,\n@\"my notes.md\" and @a.rs again @"),
        ["a.rs", "my notes.md"]
    );
    // An unclosed quote is not a mention, nor a quote that spans lines.
    assert!(mention_tokens("@\"never closed").is_empty());
    assert!(mention_tokens("@\"two\nlines\"").is_empty());
    let many = (0..PROBE_COUNT + 5)
        .map(|n| format!("@f{n}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(mention_tokens(&many).len(), PROBE_COUNT);
}

#[test]
fn files_embed_when_offered_and_link_otherwise() {
    let root = Root::new();
    root.file("src/main.rs", b"fn main() {}");
    let roots = [root.workspace("r")];
    let embedded = resolve(&tokens("Explain @src/main.rs."), &roots, true, FILE_BYTES);
    let [block] = blocks(&embedded, true).try_into().unwrap();
    assert_eq!(block["type"], "resource");
    assert_eq!(block["resource"]["text"], "fn main() {}");
    let uri = block["resource"]["uri"].as_str().unwrap().to_owned();
    assert!(uri.starts_with("file://") && uri.ends_with("/src/main.rs"));
    // A session that stops offering embedded context gets the link instead.
    let [link] = blocks(&embedded, false).try_into().unwrap();
    assert_eq!(
        link,
        serde_json::json!({"type":"resource_link","uri":uri,"name":"main.rs","size":12})
    );
    let linked = resolve(&tokens("@src/main.rs"), &roots, false, FILE_BYTES);
    assert_eq!(blocks(&linked, true), [link]);
}

#[test]
fn only_files_inside_a_root_are_mentioned() {
    let outer = Root::new();
    outer.file("secret.txt", b"outside");
    let root = Root::new();
    root.file("dir/inner.txt", b"inside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outer.0.join("secret.txt"), root.0.join("link.txt")).unwrap();
    let roots = [root.workspace("r")];
    let text = "@dir @missing.txt @../secret.txt @/etc/hosts @./dir/inner.txt @link.txt";
    assert_eq!(
        resolve(&tokens(text), &roots, true, FILE_BYTES),
        Mentions::default()
    );
}

#[test]
fn credential_and_oversized_files_are_linked_and_the_budget_bounds_embedding() {
    let root = Root::new();
    root.file(".env", b"TOKEN=1")
        .file("big.txt", &vec![b'a'; FILE_BYTES + 1])
        .file("binary.bin", &[0xff, 0xfe])
        .file("a.txt", b"12345")
        .file("b.txt", b"67890");
    let roots = [root.workspace("r")];
    let all = resolve(
        &tokens("@.env @big.txt @binary.bin @a.txt @b.txt"),
        &roots,
        true,
        8,
    );
    let kinds: Vec<_> = blocks(&all, true)
        .iter()
        .map(|block| block["type"].as_str().unwrap().to_owned())
        .collect();
    // Only a.txt fits the 8 bytes left; b.txt would pass them.
    assert_eq!(
        kinds,
        [
            "resource_link",
            "resource_link",
            "resource_link",
            "resource",
            "resource_link"
        ]
    );
    assert!(all.retained_bytes() >= 5);
}

#[test]
fn roots_resolve_in_order_and_repeats_and_extra_files_are_dropped() {
    let first = Root::new();
    first.file("same.txt", b"first");
    let second = Root::new();
    second.file("same.txt", b"second").file("only.txt", b"only");
    for n in 0..=MENTION_COUNT {
        second.file(&format!("f{n}.txt"), b"x");
    }
    let roots = [first.workspace("a"), second.workspace("b")];
    let found = resolve(
        &tokens("@same.txt @only.txt @only.txt."),
        &roots,
        true,
        FILE_BYTES,
    );
    let texts: Vec<_> = blocks(&found, true)
        .iter()
        .map(|block| block["resource"]["text"].clone())
        .collect();
    assert_eq!(texts, ["first", "only"]);
    let many = (0..=MENTION_COUNT)
        .map(|n| format!("@f{n}.txt"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        blocks(&resolve(&tokens(&many), &roots, false, 0), false).len(),
        MENTION_COUNT
    );
}

#[test]
fn prompts_send_text_then_staged_files_then_mentions() {
    let root = Root::new();
    root.file("a.txt", b"alpha");
    let mut prompt = crate::acp::intent::Prompt::new("prompt", "Read @a.txt").unwrap();
    let plain = prompt.retained_bytes();
    prompt.mentions = resolve(
        &tokens(prompt.as_str()),
        &[root.workspace("r")],
        true,
        FILE_BYTES,
    );
    assert!(prompt.retained_bytes() > plain);
    let wire = prompt.wire_blocks(true).unwrap();
    assert_eq!(
        wire[0],
        serde_json::json!({"type":"text","text":"Read @a.txt"})
    );
    assert_eq!(wire[1]["resource"]["text"], "alpha");
    assert_eq!(wire.len(), 2);
    // Mentions never make the durable prompt carry more than its text.
    assert_eq!(serde_json::to_value(&prompt).unwrap(), "Read @a.txt");
}
