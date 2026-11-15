use super::*;
use crate::acp::{
    attached_context::files::{Resource, Resources, prompt_blocks},
    transcript_quote::Prompt,
};
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
    fn root(&self, id: &str, name: &str) -> MentionRoot {
        MentionRoot {
            workspace: Workspace::open("project", id, &self.0).unwrap(),
            name: name.into(),
        }
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

fn texts(mentions: &Mentions) -> Vec<serde_json::Value> {
    blocks(mentions, true)
        .iter()
        .map(|block| block["resource"]["text"].clone())
        .collect()
}

#[test]
fn tokens_start_a_word_and_quoted_tokens_keep_their_spaces() {
    assert_eq!(
        mention_tokens("@a.rs see bob@example.com,\n@\"my notes.md\" and @a.rs again @"),
        ["a.rs", "my notes.md"]
    );
    assert_eq!(
        mention_tokens("(@b.rs) f(@c.rs) (@\"d e\")"),
        ["b.rs)", "d e"]
    );
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
    let roots = [root.root("r", "r")];
    let embedded = resolve(&tokens("Explain @src/main.rs."), &roots, true, FILE_BYTES);
    let [block] = blocks(&embedded, true).try_into().unwrap();
    assert_eq!(block["type"], "resource");
    assert_eq!(block["resource"]["text"], "fn main() {}");
    let uri = block["resource"]["uri"].as_str().unwrap().to_owned();
    assert!(uri.starts_with("file://") && uri.ends_with("/src/main.rs"));
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
    root.file("dir/inner.txt", b"inside")
        .file(".env", b"TOKEN=1");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outer.0.join("secret.txt"), root.0.join("link.txt")).unwrap();
    let roots = [root.root("r", "r")];
    let text = "@dir @missing.txt @../secret.txt @/etc/hosts @./dir/inner.txt @link.txt \
                @.env::$DATA @C:dir/inner.txt @dir/inner.txt\0";
    assert_eq!(
        resolve(&tokens(text), &roots, true, FILE_BYTES),
        Mentions::default()
    );
}

#[test]
fn ignored_files_are_not_sent() {
    let root = Root::new();
    root.file(".git/HEAD", b"ref: refs/heads/main\n")
        .file(".gitignore", b"dist/\n*.log\n")
        .file("dist/out.js", b"built")
        .file("debug.log", b"log")
        .file("node_modules/pkg/index.js", b"dep")
        .file("kept.txt", b"kept");
    let roots = [root.root("r", "r")];
    let found = resolve(
        &tokens("@dist/out.js @debug.log @node_modules/pkg/index.js @.git/HEAD @kept.txt"),
        &roots,
        true,
        FILE_BYTES,
    );
    assert_eq!(texts(&found), ["kept"]);
}

#[test]
fn credential_binary_and_oversized_files_are_linked_and_the_budget_bounds_embedding() {
    let root = Root::new();
    root.file(".env", b"TOKEN=1")
        .file("big.txt", &vec![b'a'; FILE_BYTES + 1])
        .file("binary.bin", &[0xff, 0xfe])
        .file("nul.txt", b"a\0b")
        .file("a.txt", b"12345")
        .file("b.txt", b"67890");
    let roots = [root.root("r", "r")];
    let names = tokens("@.env @big.txt @binary.bin @nul.txt @a.txt @b.txt");
    let links = resolve(&names, &roots, false, FILE_BYTES).retained_bytes();
    let all = resolve(&names, &roots, true, links + 8);
    let kinds: Vec<_> = blocks(&all, true)
        .iter()
        .map(|block| block["type"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        kinds,
        [
            "resource_link",
            "resource_link",
            "resource_link",
            "resource_link",
            "resource",
            "resource_link"
        ]
    );
    assert_eq!(all.retained_bytes(), links + 5);
}

#[test]
fn a_qualified_token_names_its_root_and_a_bare_one_takes_the_first() {
    let first = Root::new();
    first.file("same.txt", b"first");
    let second = Root::new();
    second.file("same.txt", b"second").file("only.txt", b"only");
    let third = Root::new();
    third.file("same.txt", b"third");
    let roots = [
        first.root("id-a", "app"),
        second.root("id-b", "lib"),
        third.root("id-c", "app"),
    ];
    let found = |text: &str| texts(&resolve(&tokens(text), &roots, true, FILE_BYTES));
    assert_eq!(found("@same.txt @only.txt"), ["first", "only"]);
    assert_eq!(found("@lib:same.txt"), ["second"]);
    assert_eq!(found("@\"id-c:same.txt\"."), ["third"]);
    assert!(found("@app:same.txt @lib:missing.txt @nope:same.txt").is_empty());
}

#[test]
fn repeats_of_one_file_and_files_past_the_caps_are_dropped() {
    let root = Root::new();
    root.file("a.txt", b"alpha").file("real.txt", b"real");
    fs::hard_link(root.0.join("a.txt"), root.0.join("hard.txt")).unwrap();
    for n in 0..=MENTION_COUNT {
        root.file(&format!("f{n}.txt"), b"x");
    }
    let roots = [root.root("r", "r")];
    let found = resolve(
        &tokens("@a.txt @hard.txt @a.txt."),
        &roots,
        true,
        FILE_BYTES,
    );
    assert_eq!(texts(&found), ["alpha"]);
    let many = (0..=MENTION_COUNT)
        .map(|n| format!("@f{n}.txt"))
        .collect::<Vec<_>>()
        .join(" ");
    let found = resolve(&tokens(&many), &roots, false, FILE_BYTES);
    assert_eq!(blocks(&found, false).len(), MENTION_COUNT);
    let missing = |count: usize| {
        let mut text = (0..count).map(|n| format!("@m{n}. ")).collect::<String>();
        text.push_str("@real.txt");
        blocks(&resolve(&tokens(&text), &roots, false, FILE_BYTES), false).len()
    };
    assert_eq!(missing(PROBE_COUNT / 2 - 1), 1);
    assert_eq!(missing(PROBE_COUNT / 2), 0);
}

#[test]
fn mentions_charged_beside_staged_files_near_the_cap_still_fit_the_dispatch() {
    let root = Root::new();
    let text = [b'x'; 1024];
    root.file("a.txt", &text)
        .file("b.txt", &text)
        .file("c.txt", &text);
    let roots = [root.root("r", "r")];
    let prompt = Prompt::new("prompt", "Read @a.txt @b.txt @c.txt").unwrap();
    let names = tokens(prompt.as_str());
    let link = resolve(&names[..1], &roots, false, FILE_BYTES).retained_bytes();
    let staged = |bytes: usize| {
        Resources(Arc::new(vec![Resource {
            id: "f".into(),
            name: "n".into(),
            bytes: vec![b'x'; bytes],
        }]))
    };
    let left = 2 * link + 3;
    let files = staged(prompt.mention_budget(&staged(0)) - left);
    assert_eq!(prompt.mention_budget(&files), left);
    for (budget, sent) in [(left, 2), (link - 1, 0)] {
        let mut sending = prompt.clone();
        sending.mentions = resolve(&names, &roots, true, budget);
        assert_eq!(blocks(&sending.mentions, true).len(), sent);
        assert!(sending.fits_dispatch(&files));
    }
}

#[test]
fn prompts_send_text_then_staged_files_then_mentions() {
    let root = Root::new();
    root.file("a.txt", b"alpha");
    let mut prompt = Prompt::new("prompt", "Read @a.txt").unwrap();
    let plain = prompt.retained_bytes();
    prompt.mentions = resolve(
        &tokens(prompt.as_str()),
        &[root.root("r", "r")],
        true,
        FILE_BYTES,
    );
    assert!(prompt.retained_bytes() > plain);
    let capabilities: sage_acp_protocol::intent::CapabilitySnapshotV1 = serde_json::from_str(
        r#"{"schemaVersion":1,"protocolVersion":"1","newSession":true,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":true,"promptResources":true,"cancel":true,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#,
    )
    .unwrap();
    let wire = prompt_blocks(&prompt, Some(&capabilities)).unwrap();
    assert_eq!(
        wire[0],
        serde_json::json!({"type":"text","text":"Read @a.txt"})
    );
    assert_eq!(wire[1]["resource"]["text"], "alpha");
    assert_eq!(wire.len(), 2);
    assert_eq!(serde_json::to_value(&prompt).unwrap(), "Read @a.txt");
}
