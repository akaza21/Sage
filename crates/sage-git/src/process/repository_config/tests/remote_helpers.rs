use super::*;

#[test]
fn fetch_refuses_a_repository_selected_vcs_before_running_its_alias() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    git(
        &root,
        &[
            "config",
            "remote.origin.url",
            "https://example.invalid/repo",
        ],
    );
    git(&root, &["config", "remote.origin.vcs", "sage-review"]);
    git(
        &root,
        &[
            "config",
            "alias.remote-sage-review",
            "!echo ran > helper-ran",
        ],
    );

    let message = refusal(sage_git(&root, &["fetch", "origin"]));
    assert!(message.contains("remote.origin.vcs"), "{message}");
    assert!(!root.join("helper-ran").exists());

    // Control: ordinary Git falls back to the shell alias for this helper.
    let output = crate::process::fixture_git()
        .arg("-C")
        .arg(&root)
        .args(["fetch", "origin"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        root.join("helper-ran").exists(),
        "fixture helper is not live"
    );
}

#[test]
fn remote_helper_urls_and_rewrites_cannot_enable_unadmitted_protocols() {
    for (case, key, value, push) in [
        ("explicit", "remote.origin.url", "sage-review::repo", false),
        ("scheme", "remote.origin.url", "sage-review://repo", false),
        (
            "pushurl",
            "remote.origin.pushurl",
            "sage-review::repo",
            true,
        ),
        (
            "rewrite",
            "url.sage-review::.insteadOf",
            "https://example.invalid/",
            false,
        ),
        (
            "push-rewrite",
            "url.sage-review::.pushInsteadOf",
            "https://example.invalid/",
            true,
        ),
    ] {
        let scratch = Scratch::new();
        let root = scratch.repository(case);
        git(
            &root,
            &[
                &IDENTITY[..],
                &["commit", "-q", "--allow-empty", "-m", "source"],
            ]
            .concat(),
        );
        git(
            &root,
            &[
                "config",
                "remote.origin.url",
                "https://example.invalid/repo",
            ],
        );
        git(&root, &["config", key, value]);
        // A default-only `protocol.allow=never` can be undone by this key.
        git(&root, &["config", "protocol.allow", "always"]);
        git(&root, &["config", "protocol.sage-review.allow", "always"]);
        let args: &[&str] = if push {
            &["push", "origin", "HEAD:refs/heads/main"]
        } else {
            &["fetch", "origin"]
        };

        let output = sage_git(&root, args).unwrap();
        assert!(!output.succeeded(), "{case}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("transport 'sage-review' not allowed"),
            "{case}: {stderr}"
        );
        assert!(!root.join("helper-ran").exists());

        // The fixture is live: without Sage's invocation policy, Git reaches
        // this real shell alias. Keep it on the control's command line so
        // repository alias refusal cannot mask the transport restriction.
        let output = crate::process::fixture_git()
            .arg("-C")
            .arg(&root)
            .args(["-c", "alias.remote-sage-review=!echo ran > helper-ran"])
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            root.join("helper-ran").exists(),
            "{case}: fixture helper is not live"
        );
    }
}

#[test]
fn admitted_protocol_names_cannot_dispatch_repository_helper_aliases() {
    for helper in ["ssh", "file", "git"] {
        let scratch = Scratch::new();
        let root = scratch.repository("repo");
        git(
            &root,
            &["config", "remote.origin.url", &format!("{helper}::repo")],
        );
        git(
            &root,
            &[
                "config",
                &format!("alias.remote-{helper}"),
                "!echo ran > helper-ran",
            ],
        );

        let message = refusal(sage_git(&root, &["fetch", "origin"]));
        assert!(message.contains("alias.remote-*"), "{message}");
        assert!(!root.join("helper-ran").exists());

        let output = crate::process::fixture_git()
            .arg("-C")
            .arg(&root)
            .args(["fetch", "origin"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            root.join("helper-ran").exists(),
            "{helper}: fixture helper is not live"
        );
    }
}

#[test]
fn helper_alias_refusals_redact_and_bound_repository_controlled_names() {
    use crate::public_error::MAX_PUBLIC_REPOSITORY_ERROR_BYTES;

    for name in [
        "remote-review.secret-token".to_owned(),
        format!("remote-{}", "x".repeat(1024)),
    ] {
        let scratch = Scratch::new();
        let root = scratch.repository("repo");
        git(
            &root,
            &["config", &format!("alias.{name}"), "!echo secret-command"],
        );
        let message = refusal(sage_git(&root, &["status", "--porcelain"]));
        assert!(message.contains("alias.remote-*"), "{message}");
        assert!(!message.contains("secret"), "{message}");
        assert!(message.len() <= MAX_PUBLIC_REPOSITORY_ERROR_BYTES);
    }
}

#[test]
fn repository_vcs_is_refused_even_for_an_admitted_protocol_and_worktree_config() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    git(&root, &["config", "extensions.worktreeConfig", "true"]);
    git(
        &root,
        &[
            "config",
            "--worktree",
            "remote.https://private-token.vcs",
            "https",
        ],
    );
    let message = refusal(sage_git(&root, &["status", "--porcelain"]));
    assert!(message.contains("remote.*.vcs"), "{message}");
    assert!(!message.contains("private-token"), "{message}");
    assert!(!message.contains("https"), "{message}");
}

#[test]
fn aliases_cannot_shadow_the_builtins_sage_invokes() {
    let scratch = Scratch::new();
    let root = scratch.repository("repo");
    for builtin in ["config", "rev-parse", "status", "fetch", "push", "remote"] {
        git(
            &root,
            &[
                "config",
                &format!("alias.{builtin}"),
                "!echo ran > alias-ran",
            ],
        );
    }
    git(&root, &["remote", "add", "origin", "../missing"]);
    for args in [
        vec!["status", "--porcelain"],
        vec!["config", "--get", "remote.origin.url"],
        vec!["rev-parse", "--show-toplevel"],
        vec!["remote"],
    ] {
        assert!(sage_git(&root, &args).unwrap().succeeded());
    }
    assert!(!sage_git(&root, &["fetch", "origin"]).unwrap().succeeded());
    assert!(
        !sage_git(&root, &["push", "origin", "HEAD"])
            .unwrap()
            .succeeded()
    );
    assert!(!root.join("alias-ran").exists());
}

#[test]
fn ordinary_https_ssh_and_file_remotes_are_admitted_and_file_fetches_succeed() {
    let scratch = Scratch::new();
    let source = scratch.repository("source");
    git(
        &source,
        &[
            &IDENTITY[..],
            &["commit", "-q", "--allow-empty", "-m", "source"],
        ]
        .concat(),
    );
    let root = scratch.repository("repo");
    let file_url = url::Url::from_directory_path(&source).unwrap().to_string();
    for (remote, remote_url) in [
        ("https", "https://example.invalid/repo"),
        ("ssh", "ssh://example.invalid/repo"),
        ("scp", "example.invalid:repo"),
        ("file", file_url.as_str()),
        ("local", "../source"),
    ] {
        git(&root, &["remote", "add", remote, remote_url]);
        let output = sage_git(&root, &["remote", "get-url", remote]).unwrap();
        assert!(output.succeeded());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), remote_url);
    }
    for remote in ["file", "local"] {
        let output = sage_git(&root, &["fetch", "--no-tags", remote]).unwrap();
        assert!(
            output.succeeded(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = sage_git(&root, &["rev-parse", "--verify", "FETCH_HEAD"]).unwrap();
        assert!(output.succeeded());
        assert_eq!(
            output.stdout,
            sage_git(&source, &["rev-parse", "HEAD"]).unwrap().stdout
        );
    }
}

#[test]
fn admitted_network_transports_reach_the_remote() {
    use std::{io::ErrorKind, net::TcpListener, thread, time::Instant};

    for protocol in ["http", "https", "ssh", "git"] {
        let scratch = Scratch::new();
        let root = scratch.repository("repo");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        git(
            &root,
            &[
                "remote",
                "add",
                "origin",
                &format!("{protocol}://{address}/repo"),
            ],
        );
        // Accept and close a real connection: this verifies dispatch without
        // external network access, certificates, credentials, or a mock Git.
        let server = thread::spawn(move || {
            let deadline = Instant::now() + budget::READ;
            loop {
                match listener.accept() {
                    Ok(_) => return true,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return false;
                        }
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => panic!("loopback listener failed: {error}"),
                }
            }
        });
        // The ssh transport runs an `ssh` program, which a machine need not
        // have. This test binary stands in for it: Git still selects and
        // admits the transport and passes OpenSSH's options, and the
        // stand-in opens the connection they name. Only the program is
        // chosen on the command line, which no repository config can do.
        // Not covered: that Git finds a real `ssh` once Sage clears PATH.
        let ssh_command = format!("core.sshCommand={}", ssh_stand_in());
        let mut args = vec!["ls-remote", "origin"];
        if protocol == "ssh" {
            args.splice(0..0, ["-c", "ssh.variant=ssh", "-c", ssh_command.as_str()]);
        }
        let result = sage_git(&root, &args);
        let connected = server.join().unwrap();
        let output = result.unwrap();
        assert!(
            connected,
            "{protocol}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.succeeded(),
            "the fixture closes before Git negotiation"
        );
    }
}

const SSH_STAND_IN: &str = "sage-ssh-stand-in";

/// The `core.sshCommand` that runs this test binary as `ssh`. Git appends
/// OpenSSH's arguments after `--`, where libtest takes them as name filters
/// that match no test. The executable's path is not verbatim, and MSYS
/// argument conversion is off, so Git for Windows' sh runs it unchanged.
fn ssh_stand_in() -> String {
    let executable = plain(&std::env::current_exe().unwrap()).replace('\\', "/");
    format!(
        "MSYS2_ARG_CONV_EXCL='*' '{executable}' --exact \
         process::repository_config::tests::remote_helpers::ssh_stand_in_fixture \
         --nocapture -- {SSH_STAND_IN}"
    )
}

/// Run as the ssh stand-in, connects to the host and port Git passes `ssh`
/// and closes. Run any other way, it has nothing to do.
#[test]
fn ssh_stand_in_fixture() {
    let mut arguments = std::env::args()
        .skip_while(|argument| argument != "--")
        .skip(1);
    if arguments.next().as_deref() != Some(SSH_STAND_IN) {
        return;
    }
    let mut port = 22;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-p" => port = arguments.next().unwrap().parse().unwrap(),
            "-o" => {
                arguments.next();
            }
            option if option.starts_with('-') => {}
            destination => {
                let host = destination.rsplit('@').next().unwrap();
                drop(std::net::TcpStream::connect((host, port)).unwrap());
                return;
            }
        }
    }
    panic!("Git passed the ssh stand-in no destination");
}
