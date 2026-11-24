use infra::agents::{group_members, is_managed, scan, sudoers, valid_name, Agent, Sudo};

const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITEST agent";

fn dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (n, c) in files {
        std::fs::write(d.path().join(n), c).unwrap();
    }
    d
}

#[test]
fn names() {
    for ok in ["agent-pc", "ai-devin", "_svc", "a1"] {
        assert!(valid_name(ok), "{ok}");
    }
    for bad in ["", "Agent", "1abc", "a.b", "a b", "../x", "a".repeat(33).as_str()] {
        assert!(!valid_name(bad), "{bad}");
    }
}

#[test]
fn scan_any_name_and_sudo_levels() {
    let with_nl = format!("{KEY}\n");
    let d = dir(&[
        ("agent-pc.pub", with_nl.as_str()),
        ("agent-pc.yaml", "sudo: full\n"),
        ("ai-devin.pub", KEY),
        ("ops.pub", KEY),
        ("ops.yaml", "sudo: none"),
        ("empty.pub", KEY),
        ("empty.yaml", ""),
        ("ai-github.pub", KEY),
        ("README.md", "pas une clé"),
    ]);
    let got: Vec<(String, Sudo)> = scan(d.path()).unwrap().into_iter().map(|a| (a.name, a.sudo)).collect();
    assert_eq!(
        got,
        vec![
            ("agent-pc".into(), Sudo::Full),
            ("ai-devin".into(), Sudo::Limited),
            ("ai-github".into(), Sudo::None),
            ("empty".into(), Sudo::Limited),
            ("ops".into(), Sudo::None),
        ]
    );
    assert_eq!(scan(d.path()).unwrap()[0].key, KEY);
}

#[test]
fn scan_rejects_bad_input() {
    for files in [
        vec![("Bad.pub", KEY)],
        vec![("ai-infra.pub", KEY)],
        vec![("root.pub", KEY)],
        vec![("x.pub", "")],
        vec![("x.pub", "k1\nk2")],
        vec![("x.pub", KEY), ("x.yaml", "sudo: all")],
        vec![("x.pub", KEY), ("x.yaml", "sudoo: full")],
        vec![("ai-github.pub", KEY), ("ai-github.yaml", "sudo: full")],
    ] {
        assert!(scan(dir(&files).path()).is_err(), "{files:?}");
    }
    assert!(scan(std::path::Path::new("/nonexistent/agents")).unwrap().is_empty());
}

#[test]
fn sudoers_content() {
    let a = |sudo| Agent { name: "agent-pc".into(), key: KEY.into(), sudo };
    assert_eq!(sudoers(&a(Sudo::None)), None);
    let full = sudoers(&a(Sudo::Full)).unwrap();
    assert!(full.contains("Defaults:agent-pc log_input, log_output\n"));
    assert!(full.ends_with("agent-pc ALL=(ALL:ALL) NOPASSWD: ALL\n"));
    let lim = sudoers(&a(Sudo::Limited)).unwrap();
    assert!(lim.contains("Defaults:agent-pc log_input, log_output\n"));
    assert!(lim.ends_with("agent-pc ALL=(root) NOPASSWD: /usr/bin/systemctl, /usr/bin/tailscale, /usr/bin/apt-get, /usr/local/bin/infra\n"));
}

#[test]
fn managed_accounts() {
    let etc = "root:x:0:\ninfra:x:1004:ai-infra,amonot\ninfra-agents:x:1010:agent-pc,ai-github\n";
    let m = group_members(etc, "infra-agents");
    assert_eq!(m, vec!["agent-pc", "ai-github"]);
    assert!(group_members(etc, "absent").is_empty());
    assert!(group_members("infra-agents:x:1010:\n", "infra-agents").is_empty());
    assert!(is_managed("agent-pc", &m));
    assert!(is_managed("ai-devin", &m)); // ancien compte ai-*
    assert!(!is_managed("amonot", &m));
    assert!(!is_managed("claude", &m));
    assert!(!is_managed("ai-infra", &m));
    assert!(!is_managed("root", &["root".to_string()]));
}
