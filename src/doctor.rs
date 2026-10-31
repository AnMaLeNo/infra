//! infra doctor : état de l'hôte, et (--legacy) ce qui tourne hors modèle.
use std::process::Command;

use serde_json::{json, Map, Value};

use crate::util::which;
use crate::{config, paths, secrets, stack};

/// ssh, tailscale
pub const WHITELIST_LISTEN: [&str; 2] = ["22", "41641"];

fn out(cmd: &[&str]) -> String {
    Command::new(cmd[0]).args(&cmd[1..]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn ok(cond: bool, ok: &str, ko: &str) -> Value {
    Value::String(if cond { ok } else { ko }.to_string())
}

pub fn doctor(legacy: bool) -> Map<String, Value> {
    let mut rep = Map::new();
    let host = config::host();
    rep.insert("host.yaml".into(), match &host {
        Ok(h) => json!(format!("ok ({})", h.name)),
        Err(e) => json!(format!("KO {e}")),
    });
    rep.insert("registry.yaml".into(), match config::registry() {
        Ok(r) => json!(format!("ok ({} projets)", r.projects.len())),
        Err(e) => json!(format!("KO {e}")),
    });
    for b in ["docker", "sops", "age", "tailscale", "git"] {
        rep.insert(b.into(), ok(which(b).is_some(), "ok", "KO absent"));
    }
    rep.insert("claude".into(), ok(which("claude").is_some(), "ok", "KO absent (runner inutilisable)"));
    let age = paths::age_key_file();
    rep.insert("age-key".into(), ok(age.exists(), "ok", &format!("KO {} absent", age.display())));
    if age.exists() {
        let chk = secrets::check();
        let all_ok = chk.values().all(|v| v == "ok");
        rep.insert("secrets".into(), ok(all_ok, &format!("ok ({})", chk.len()), &format!("KO {}", json!(chk))));
    }
    if let Ok(h) = &host {
        let ts = out(&["tailscale", "ip", "-4"]).trim().to_string();
        rep.insert(
            "tailscale_ip".into(),
            ok(ts == h.tailscale_ip, &format!("ok {ts}"), &format!("KO réel={} attendu={}", if ts.is_empty() { "aucune" } else { &ts }, h.tailscale_ip)),
        );
        let nets = out(&["docker", "network", "ls", "--format", "{{.Name}}"]);
        rep.insert("traefik-net".into(), ok(nets.split_whitespace().any(|n| n == "traefik-net"), "ok", "KO"));
        for s in stack::all_names().unwrap_or_default() {
            let v = match stack::status(&s) {
                Ok(st) => ok(
                    !st.is_empty() && st.values().all(|v| v == "running" || v == "healthy"),
                    "ok",
                    &format!("KO {}", if st.is_empty() { "absent".to_string() } else { json!(st).to_string() }),
                ),
                Err(e) => json!(format!("KO {e}")),
            };
            rep.insert(format!("stack/{s}"), v);
        }
        if h.public_ingress == "cloudflare-tunnel" {
            rep.insert("tunnel_id".into(), ok(!h.cloudflare.tunnel_id.is_empty(), "ok", "KO (infra tunnel create)"));
        }
    }
    let active = out(&["systemctl", "is-active", "infra-runner"]).trim().to_string();
    rep.insert("infra-runner".into(), ok(active == "active", "ok", &format!("KO {active}")));
    // sockets exposés hors liste blanche
    let exposed: Vec<String> = out(&["ss", "-ltnH"])
        .lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            let local = *parts.get(3)?;
            let (ip, port) = local.rsplit_once(':')?;
            (matches!(ip, "0.0.0.0" | "*" | "[::]") && !WHITELIST_LISTEN.contains(&port)).then(|| local.to_string())
        })
        .collect();
    rep.insert("listen_0.0.0.0".into(), ok(exposed.is_empty(), "ok", &format!("KO {exposed:?}")));
    if legacy {
        rep.insert("legacy".into(), legacy_report());
    }
    rep
}

fn legacy_report() -> Value {
    let units = out(&["systemctl", "list-units", "--type=service", "--state=running", "--no-legend", "--plain"]);
    let std = [
        "ssh", "sshd", "docker", "containerd", "tailscaled", "infra-runner", "cron", "dbus", "systemd-", "getty@", "user@", "rsyslog",
        "polkit", "avahi", "ModemManager", "wpa_supplicant", "NetworkManager", "bluetooth", "smartd", "netdata", "unattended-upgrades",
        "earlyoom", "zramswap", "rpi-eeprom", "udisks2", "fail2ban", "accounts-daemon", "lightdm", "gdm", "cups", "colord", "upower",
        "rtkit", "triggerhappy", "thd",
    ];
    let mut su: Vec<String> = units
        .lines()
        .filter_map(|u| u.split_whitespace().next())
        .filter(|u| !std.iter().any(|s| u.starts_with(s)))
        .map(str::to_string)
        .collect();
    su.sort();
    let projects = out(&["docker", "ps", "--format", "{{.Label \"com.docker.compose.project\"}}|{{.Label \"com.docker.compose.project.working_dir\"}}"]);
    let apps = paths::apps_dir().to_string_lossy().into_owned();
    let mut outside: Vec<String> = vec![];
    for line in projects.lines() {
        let (proj, wd) = line.split_once('|').unwrap_or((line, ""));
        if !wd.is_empty() && !(wd.starts_with(&apps) || proj.starts_with("infra-")) {
            outside.push(format!("{} @ {wd}", if proj.is_empty() { "(docker run)" } else { proj }));
        }
        if proj.is_empty() {
            outside.push("(docker run sans compose)".into());
        }
    }
    outside.sort();
    outside.dedup();
    json!({"systemd_units": su, "compose_hors_srv_apps": outside})
}
