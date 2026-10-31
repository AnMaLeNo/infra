mod common;

use infra::editor;
use std::path::Path;

#[test]
fn command_serves_web_with_token_file_and_vsix_layout() {
    let e = common::env();
    std::env::set_var("HOME", e.root.path());
    let (argv, env) = editor::command_with(Path::new("/usr/local/bin/vscode-cli"), "172.30.0.1").unwrap();
    assert_eq!(&argv[..2], ["/usr/local/bin/vscode-cli", "serve-web"]);
    let after = |flag: &str| argv[argv.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(after("--host"), "172.30.0.1");
    assert!(!argv.contains(&"--without-connection-token".to_string()));
    assert_eq!(after("--cli-data-dir"), editor::data_dir().to_string_lossy());
    assert!(after("--default-folder").ends_with("config"));
    let f = e.root.path().join(".config/infra-editor/token");
    assert_eq!(after("--connection-token-file"), f.to_string_lossy());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    let first = std::fs::read_to_string(&f).unwrap();
    editor::command_with(Path::new("/usr/local/bin/vscode-cli"), "172.30.0.1").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), first);
    assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");

    let sf = editor::server_dir().join("data/Machine/settings.json");
    std::fs::create_dir_all(sf.parent().unwrap()).unwrap();
    std::fs::write(&sf, r#"{"editor.fontSize": 14}"#).unwrap();
    editor::configure().unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&sf).unwrap()).unwrap();
    assert_eq!(v, serde_json::json!({"editor.fontSize": 14, "claudeCode.claudeProcessWrapper": "/usr/bin/env"}));

    let vsix = editor::build_vsix().unwrap();
    let mut z = zip::ZipArchive::new(std::fs::File::open(vsix).unwrap()).unwrap();
    let mut names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
    names.sort();
    assert_eq!(names, ["[Content_Types].xml", "extension.vsixmanifest", "extension/extension.js", "extension/package.json"]);

    editor::request_session("FD3ACD69-38C1-4560-9A1A-F04596739D38").unwrap();
    assert_eq!(std::fs::read_to_string(infra::paths::runs_dir().join(".editor-open")).unwrap(), "fd3acd69-38c1-4560-9a1a-f04596739d38");
    assert!(editor::request_session("../../etc/passwd").is_err());
}
