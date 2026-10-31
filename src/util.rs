//! Processus externes, fichiers privés, dotenv, horodatage et JSON « à la Python » (hash compatible).
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CmdError(pub String);

#[derive(Debug, Default, Clone)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

#[derive(Default)]
pub struct Cmd<'a> {
    pub cwd: Option<&'a Path>,
    pub env: Option<&'a HashMap<String, String>>,
    pub check: bool,
    pub capture: bool,
    pub input: Option<&'a str>,
    pub quiet: bool,
}

impl<'a> Cmd<'a> {
    pub fn new() -> Self {
        Cmd { check: true, capture: true, ..Default::default() }
    }
    pub fn cwd(mut self, p: &'a Path) -> Self {
        self.cwd = Some(p);
        self
    }
    pub fn env(mut self, e: &'a HashMap<String, String>) -> Self {
        self.env = Some(e);
        self
    }
    pub fn no_check(mut self) -> Self {
        self.check = false;
        self
    }
    pub fn no_capture(mut self) -> Self {
        self.capture = false;
        self
    }
    pub fn input(mut self, s: &'a str) -> Self {
        self.input = Some(s);
        self
    }
    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    pub fn run<S: AsRef<OsStr>>(self, cmd: &[S]) -> Result<Output> {
        let words: Vec<String> = cmd.iter().map(|c| c.as_ref().to_string_lossy().into_owned()).collect();
        if !self.quiet {
            eprintln!("$ {}", shell_join(&words));
        }
        let mut c = Command::new(&words[0]);
        c.args(&words[1..]);
        if let Some(d) = self.cwd {
            c.current_dir(d);
        }
        if let Some(e) = self.env {
            c.envs(e);
        }
        c.stdin(if self.input.is_some() { Stdio::piped() } else { Stdio::inherit() });
        if self.capture {
            c.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let mut child = c.spawn().map_err(|e| CmdError(format!("impossible de lancer {} : {e}", words[0])))?;
        if let Some(inp) = self.input {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(inp.as_bytes());
            }
        }
        let out = child.wait_with_output()?;
        let res = Output {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        };
        if self.check && res.code != 0 {
            return Err(CmdError(format!(
                "commande échouée ({}) : {}\n{}",
                res.code,
                words.join(" "),
                res.stderr.trim()
            ))
            .into());
        }
        Ok(res)
    }
}

pub fn shell_join(words: &[String]) -> String {
    words.iter().map(|w| shell_quote(w)).collect::<Vec<_>>().join(" ")
}

pub fn shell_quote(w: &str) -> String {
    if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c)) {
        w.to_string()
    } else {
        format!("'{}'", w.replace('\'', "'\"'\"'"))
    }
}

pub fn run<S: AsRef<OsStr>>(cmd: &[S]) -> Result<Output> {
    Cmd::new().run(cmd)
}

/// `docker compose ps --format json` renvoie parfois un objet par ligne.
pub fn run_json<S: AsRef<OsStr>>(cmd: &[S], cwd: Option<&Path>, env: Option<&HashMap<String, String>>) -> Result<Vec<Value>> {
    let mut c = Cmd::new().quiet();
    if let Some(d) = cwd {
        c = c.cwd(d);
    }
    if let Some(e) = env {
        c = c.env(e);
    }
    let out = c.run(cmd)?.stdout.trim().to_string();
    if out.is_empty() {
        return Ok(vec![]);
    }
    match serde_json::from_str::<Value>(&out) {
        Ok(Value::Array(a)) => Ok(a),
        Ok(v) => Ok(vec![v]),
        Err(_) => out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<Value>(l).map_err(Into::into))
            .collect(),
    }
}

/// Écrit si différent. Retourne true si le fichier a changé.
pub fn write_private(p: &Path, content: &str, mode: u32) -> Result<bool> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    if p.exists() && fs::read_to_string(p)? == content {
        fs::set_permissions(p, fs::Permissions::from_mode(mode))?;
        return Ok(false);
    }
    let tmp = tmp_path(p);
    fs::write(&tmp, content)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
    fs::rename(&tmp, p)?;
    Ok(true)
}

fn tmp_path(p: &Path) -> PathBuf {
    let mut name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name.push_str(".tmp");
    p.with_file_name(name)
}

pub fn read_or_empty(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_default()
}

pub fn parse_dotenv(text: &str) -> indexmap::IndexMap<String, String> {
    let mut env = indexmap::IndexMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (k, v) = line.split_once('=').unwrap();
        let k = k.trim().strip_prefix("export ").unwrap_or(k.trim()).trim();
        let mut v = v.trim();
        let b = v.as_bytes();
        if b.len() >= 2 && b[0] == b[b.len() - 1] && (b[0] == b'"' || b[0] == b'\'') {
            v = &v[1..v.len() - 1];
        }
        env.insert(k.to_string(), v.to_string());
    }
    env
}

pub fn dump_dotenv(env: &indexmap::IndexMap<String, String>) -> String {
    env.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
}

/// `datetime.now(UTC).isoformat()` : 2026-10-31T12:34:56.789012+00:00
pub fn now_iso() -> String {
    iso(Utc::now())
}

pub fn iso(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
}

pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(bin)).find(|p| p.is_file())
}

/// `json.dumps(v, sort_keys=True)` de Python, à l'octet près (ensure_ascii, séparateurs « , » et « : »),
/// pour que les empreintes restent identiques à celles déjà enregistrées par la version Python.
pub fn py_json_sorted(v: &Value) -> String {
    let mut s = String::new();
    py_json_into(v, &mut s);
    s
}

fn py_json_into(v: &Value, s: &mut String) {
    match v {
        Value::Null => s.push_str("null"),
        Value::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(f) = n.as_f64().filter(|_| !n.is_i64() && !n.is_u64()) {
                s.push_str(&py_float(f));
            } else {
                s.push_str(&n.to_string());
            }
        }
        Value::String(t) => py_str(t, s),
        Value::Array(a) => {
            s.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                py_json_into(x, s);
            }
            s.push(']');
        }
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            s.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                py_str(k, s);
                s.push_str(": ");
                py_json_into(&o[*k], s);
            }
            s.push('}');
        }
    }
}

fn py_float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

fn py_str(t: &str, s: &mut String) {
    s.push('"');
    for c in t.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            '\u{8}' => s.push_str("\\b"),
            '\u{c}' => s.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    s.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

pub fn bail_if<T>(cond: bool, msg: impl Into<String>) -> Result<Option<T>> {
    if cond {
        Err(anyhow!(msg.into()))
    } else {
        Ok(None)
    }
}
